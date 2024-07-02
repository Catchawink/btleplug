use std::{pin::Pin, sync::Arc, time::Duration};
use crate::{common::adapter_manager::AdapterManager, Error, Result};
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
use bimap::BiMap;

#[derive(Clone, Debug)]
pub struct Adapter {
    manager: Arc<AdapterManager<Peripheral>>,
    ids: Arc<Mutex<BiMap<Uuid, String>>>
}

impl Adapter {
    pub(crate) async fn new() -> Result<Self> {
        let manager = Arc::new(AdapterManager::default());

          let nav = web_sys::window().unwrap().navigator();
          if nav.bluetooth().is_none() {
            log!("WebBluetooth is not supported on this browser");
            return Err(Error::NotSupported(
              "WebBluetooth is not supported on this browser".to_string(),
            ));
          }

        Ok(Adapter {
            manager,
            ids: Arc::new(Mutex::new(Default::default()))
        })
    }
}

#[async_trait]
impl Central for Adapter {
	type Peripheral = Peripheral;

    async fn events(&self) -> Result<Pin<Box<dyn Stream<Item = CentralEvent> + Send>>> {
      Ok(self.manager.event_stream())
    }

    async fn start_scan(&self, filter: ScanFilter) -> Result<()> {

      let (tx, mut rx) = oneshot::channel::<()>();

      let manager_clone = self.manager.clone();
      let ids = self.ids.clone();
      spawn_local(async move {
          let arr = Array::new();

          let mut options = web_sys::RequestDeviceOptions::new();
          options.accept_all_devices(true);

          for service_uuid in filter.services {
            arr.push(&JsValue::from(service_uuid.to_string()));
          }
          options.optional_services(&arr);

          // Uses get_devices() rather than request_device()--but get_devices() is experimental currently
          //let devices = JsFuture::from(get_bluetooth_api().get_devices()).await.map_err(|x| { Error::RuntimeError(x.as_string().unwrap()) }).expect("Failed to find devices!");
          //let devices = js_sys::Array::from(&devices).iter().map(|x| Into::<BluetoothDevice>::into(x));
          let devices = vec![BluetoothDevice::from(JsFuture::from(get_bluetooth_api().request_device(&options)).await.map_err(|x| { Error::RuntimeError(format!(
            "Error while trying to request device: {:?}",
            x
          )) }).expect("Failed to find devices!"))];

          for device in devices {
            log!("Found bluetooth device.");

            if let Some(name) = device.name() {
              log!(format!("Bluetooth device name: {}", name));
            }

            log!(format!("Bluetooth device id: {}", device.id()));
            
            let mut _id: Option<Uuid> = None;
            if let Some(value) = ids.lock().await.get_by_right(&device.id()) {
              _id = Some(value.clone());
            } else {
              let id = Uuid::new_v4();
              ids.lock().await.insert(id.clone(), device.id());
              _id = Some(id);
            }

            // Can't get device address (as on other platforms)--devices have unique IDs instead
            let id = _id.unwrap();
            
            if let Some(mut entry) = manager_clone.peripheral_mut(&id.into()) {
                entry.value_mut().update_properties(device).await;
                manager_clone.emit(CentralEvent::DeviceUpdated(id.into()));
            } else {
                let peripheral = Peripheral::new(Arc::downgrade(&manager_clone), id);
                peripheral.update_properties(device).await;
                manager_clone.add_peripheral(peripheral);
                manager_clone.emit(CentralEvent::DeviceDiscovered(id.into()));
            }
          }
          tx.send(()).unwrap();
      });

      rx.await.unwrap();

      //while let Err(_) = rx.try_recv() {
      //  super::utils::sleep(Duration::from_millis(100)).await;
      //}

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