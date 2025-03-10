use std::{cell::RefCell, collections::HashMap, pin::Pin, sync::Arc, time::Duration};
use crate::{common::adapter_manager::AdapterManager, web::tauri, Error, Result};
use super::peripheral::{Peripheral, PeripheralId};
use crate::api::{Central, CentralEvent, ScanFilter};
use async_trait::async_trait;
use futures::Stream;
use gloo_console::{error, log};
use tokio::{sync::Mutex, task::spawn_blocking};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use uuid::Uuid;
use wasm_bindgen::JsValue;
use web_sys::{BluetoothDevice, BluetoothRemoteGattCharacteristic, BluetoothRemoteGattServer, BluetoothRemoteGattService, RequestDeviceOptions};
use std::str::FromStr;
use js_sys::Array;
use super::utils::*;
use futures::channel::oneshot;
use bimap::{BiHashMap, BiMap};
use tauri_sys;

thread_local! {
  pub static DEVICES: RefCell<HashMap<String, BluetoothDevice>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Debug)]
pub struct Adapter {
    manager: Arc<AdapterManager<Peripheral>>,
    ids: Arc<std::sync::Mutex<BiMap<Uuid, String>>>
}

impl Adapter {
    pub(crate) async fn new() -> Result<Self> {
        let manager = Arc::new(AdapterManager::default());

        if !is_tauri() {
          let nav = web_sys::window().unwrap().navigator();
          if nav.bluetooth().is_none() {
            log!("WebBluetooth is not supported on this browser");
            return Err(Error::NotSupported(
              "WebBluetooth is not supported on this browser".to_string(),
            ));
          }
        }

        Ok(Adapter {
            manager,
            ids: Arc::new(std::sync::Mutex::new(Default::default()))
        })
    }
}

fn add_or_get_uuid(ids: Arc<std::sync::Mutex<BiHashMap<Uuid, String>>>, device_id: String) -> Uuid {
  let mut ids = ids.lock().unwrap();
  if let Some(value) = ids.get_by_right(&device_id) {
    value.clone()
  } else {
    let id = Uuid::new_v4();
    ids.insert(id.clone(), device_id);
    id
  }
}

#[async_trait]
impl Central for Adapter {
	type Peripheral = Peripheral;

    async fn events(&self) -> Result<Pin<Box<dyn Stream<Item = CentralEvent> + Send>>> {
      Ok(self.manager.event_stream())
    }

    async fn start_scan(&self, filter: ScanFilter) -> Result<()> {

      if is_tauri() {

        //let manager_clone = self.manager.clone();
        //let ids = self.ids.clone();

        let manager_clone = self.manager.clone();
        let ids = self.ids.clone();
        
        let (tx, rx) = oneshot::channel::<()>();

        spawn_local(async move {
          tauri::start_scan(move |devices| {
            for device in devices {
              let id = device.address;
              let uuid = add_or_get_uuid(ids.clone(), id.clone());
              if let Some(mut entry) = manager_clone.peripheral_mut(&uuid.into()) {
                // TODO: Update peripheral if it already is registered by the manager
              } else {
                log!(format!("Bluetooth device name: {}", device.name));
                let peripheral = Peripheral::new(Arc::downgrade(&manager_clone), uuid, id, Some(device.name), device.services);
                //peripheral.update_properties().await;
                manager_clone.add_peripheral(peripheral);
                manager_clone.emit(CentralEvent::DeviceDiscovered(uuid.into()));
              }
            }
          }, None, filter.services).await.expect("Failed to scan using Tauri");

          tx.send(()).unwrap();
        });

        rx.await.unwrap();
        
        log!(format!("Done scanning."));
      } else {
        let (tx, rx) = oneshot::channel::<()>();

        let manager_clone = self.manager.clone();
        let ids = self.ids.clone();
        spawn_local(async move {
            let arr = Array::new();
  
            let mut options = web_sys::RequestDeviceOptions::new();
            options.set_accept_all_devices(true);
  
            for service_uuid in filter.services {
              arr.push(&JsValue::from(service_uuid.to_string()));
            }
            options.set_optional_services(&arr);
  
            // Uses get_devices() rather than request_device()--but get_devices() is experimental currently
            //let devices = JsFuture::from(get_bluetooth_api().get_devices()).await.map_err(|x| { Error::RuntimeError(x.as_string().unwrap()) }).expect("Failed to find devices!");
            //let devices = js_sys::Array::from(&devices).iter().map(|x| Into::<BluetoothDevice>::into(x));
            let devices = vec![BluetoothDevice::from(JsFuture::from(get_bluetooth_api().request_device(&options)).await.map_err(|x| { Error::RuntimeError(format!(
              "Error while trying to request device: {:?}",
              x
            )) }).expect("Failed to find devices!"))];
  
            for device in devices {
              log!("Found bluetooth device.");
  
              let name = device.name();
              if let Some(name) = &name {
                log!(format!("Bluetooth device name: {}", name));
              }
  
              log!(format!("Bluetooth device id: {}", device.id()));
              
              let id = device.id();
              
              let uuid = add_or_get_uuid(ids.clone(), id.clone());
  
              // Can't get device address (as on other platforms)--devices have unique IDs instead
              //let id = _id.unwrap();
              
              if let Some(mut entry) = manager_clone.peripheral_mut(&uuid.into()) {
                log!(format!("Device found, updating properties."));
  
                entry.value_mut().update_properties().await;
                manager_clone.emit(CentralEvent::DeviceUpdated(uuid.into()));
              } else {
                DEVICES.with_borrow_mut(|devices| {
                  devices.insert(id.clone(), device);
                });
  
                log!(format!("Device not found, updating properties."));
            
                let peripheral = Peripheral::new(Arc::downgrade(&manager_clone), uuid, id, name, vec![]);
                peripheral.update_properties().await;
                manager_clone.add_peripheral(peripheral);
                manager_clone.emit(CentralEvent::DeviceDiscovered(uuid.into()));
              }
            }
            tx.send(()).unwrap();
        });
  
        rx.await.unwrap();
        log!(format!("Done scanning."));
      }

      Ok(())
    }

    async fn stop_scan(&self) -> Result<()> {
      // Need to implement method of cancelling spawned WASM future
		  todo!()
    }

    async fn peripherals(&self) -> Result<Vec<Peripheral>> {
      Ok(self.manager.peripherals())
    }

    async fn peripheral(&self, id: &PeripheralId) -> Result<Peripheral> {
      self.manager.peripheral(id).ok_or(Error::DeviceNotFound)
    }

    async fn add_peripheral(&self, _address: &PeripheralId) -> Result<Peripheral> {
      Err(Error::NotSupported(
        "Can't add a Peripheral from a BDAddr".to_string(),
      ))
    }

    async fn adapter_info(&self) -> Result<String> {
      Ok("WebBluetooth".to_string())
    }
}