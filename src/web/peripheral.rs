use std::{collections::{BTreeSet, HashMap}, default, fmt::{self, Debug, Display, Formatter}, pin::Pin, str::FromStr, sync::{Arc, Mutex}};
use async_trait::async_trait;
use js_sys::{Array, DataView};
use tokio::{sync::broadcast, task::spawn_blocking};
use uuid::Uuid;
use futures::{channel::mpsc::{Receiver, SendError, Sender}, Stream};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{BluetoothDevice, BluetoothRemoteGattCharacteristic, BluetoothRemoteGattDescriptor, BluetoothRemoteGattServer, BluetoothRemoteGattService, DomException};
use crate::{
    api::{
        self, BDAddr, CentralEvent, CharPropFlags, Characteristic, Descriptor,
        PeripheralProperties, Service, ValueNotification, WriteType,
    },
    common::{adapter_manager::AdapterManager, util::notifications_stream_from_broadcast_receiver},
    Error, Result,
};
use std::sync::Weak;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use gloo_console::log;

use super::utils;

#[derive(Clone)]
pub struct Peripheral {
	shared: Arc<Shared>,
}

impl Peripheral {
  pub(crate) fn new(manager: Weak<AdapterManager<Self>>, uuid: Uuid, id: String, name: Option<String>) -> Self {
    //let obj = JPeripheral::new(env, adapter, addr)?;

    let properties = Mutex::from(PeripheralProperties {
      address: BDAddr::default(),
      address_type: None,
      local_name: name,
      tx_power_level: None,
      rssi: None,
      manufacturer_data: HashMap::new(),
      service_data: HashMap::new(),
      services: Vec::new(),
      class: None,
    });

    Self {
        //addr,
        //internal: env.new_global_ref(obj)?,
        shared: Arc::new(Shared {
            //device: tokio::sync::Mutex::new(Some(device)),
            //notifications_channel: todo!(),
            manager,
            uuid: uuid,
            id: id,
            services: Default::default(),
            properties: properties,
        }),
    }
  }

  pub(crate) async fn update_properties(&self) {

    let device = utils::get_bluetooth_device(self.shared.id.clone()).await.unwrap();

    let connect_future = device.gatt().unwrap().connect();
    log!("Connecting to device...");

    let server: BluetoothRemoteGattServer = match JsFuture::from(connect_future).await {
      Ok(val) => {
        log!("Connected to device.");
        val.into()
      },
      Err(_) => {
        log!("Failed to connect to device.");
        return;
      }
    };

    let _services: Array = match JsFuture::from(server.get_primary_services()).await {
        Ok(val) => {
          val.into()
        },
        Err(e) => {
          log!(&format!("Error getting bluetooth services: {:?}", e));
          return;
        },
    };

    let mut services = self.shared.services.lock().unwrap();

    for _service in _services {
      let _service: BluetoothRemoteGattService = _service.into();

      let _characteristics: Array = match JsFuture::from(_service.get_characteristics()).await {
        Ok(val) => {
          val.into()
        },
        Err(e) => {
          log!(&format!("Error getting bluetooth characteristics: {:?}", e));
          return;
        },
      };

      let _characteristics = _characteristics.iter().map(|x| Into::<BluetoothRemoteGattCharacteristic>::into(x));

      let mut characteristics = BTreeSet::<Characteristic>::default();
      for _characteristic in _characteristics {
        let _descriptors: Array = match JsFuture::from(_characteristic.get_descriptors()).await {
          Ok(val) => {
            log!("GOT DESCRIPTORS");
            val.into()
          },
          Err(e) => {
            let exception: DomException = e.into();
            log!(exception.name());
            if exception.name() == "NotFoundError" {
              Array::new()
            } else {
              log!(&format!("Error getting bluetooth characteristic descriptors: {:?}", exception));
              Array::new()
            }
          }   
        };

        let _descriptors = _descriptors.iter().map(|x| Into::<BluetoothRemoteGattDescriptor>::into(x));
        
        let _properties = _characteristic.properties();

        let mut properties = CharPropFlags::empty();
        if _properties.broadcast() {
          properties.insert(CharPropFlags::BROADCAST);
        }
        if _properties.read() {
          properties.insert(CharPropFlags::READ);
        }
        if _properties.write_without_response() {
          properties.insert(CharPropFlags::WRITE_WITHOUT_RESPONSE);
        }
        if _properties.write() {
          properties.insert(CharPropFlags::WRITE);
        }
        if _properties.notify() {
          properties.insert(CharPropFlags::NOTIFY);
        }
        if _properties.indicate() {
          properties.insert(CharPropFlags::INDICATE);
        }
        if _properties.authenticated_signed_writes() {
          properties.insert(CharPropFlags::AUTHENTICATED_SIGNED_WRITES);
        }

        let mut descriptors = BTreeSet::<Descriptor>::default();
        for _descriptor in _descriptors {
          let descriptor = Descriptor {
            uuid: Uuid::from_str(&_descriptor.uuid()).unwrap(),
            service_uuid: Uuid::from_str(&_service.uuid()).unwrap(),
            characteristic_uuid: Uuid::from_str(&_descriptor.characteristic().uuid()).unwrap(),
          };
          descriptors.insert(descriptor);
        }

        characteristics.insert(Characteristic {
            uuid: Uuid::from_str(&_characteristic.uuid()).unwrap(),
            service_uuid: Uuid::from_str(&_service.uuid()).unwrap(),
            properties: properties,
            descriptors: descriptors,
        });
      }

      services.insert(Service {
        uuid: Uuid::from_str(&_service.uuid()).unwrap(),
        primary: _service.is_primary(),
        characteristics: characteristics,
      });
    }
  }
}

struct Shared {
    //device: tokio::sync::Mutex<Option<BluetoothDevice>>,
    //notifications_channel: broadcast::Sender<ValueNotification>,
    manager: Weak<AdapterManager<Peripheral>>,
    uuid: Uuid,
    id: String,
    services: Mutex<BTreeSet<Service>>,
    properties: Mutex<PeripheralProperties>,
    //message_sender: Sender<CoreBluetoothMessage>,
    // We're not actually holding a peripheral object here, that's held out in
    // the objc thread. We'll just communicate with it through our
    // receiver/sender pair.
}

impl Display for Peripheral {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        // let connected = if self.is_connected() { " connected" } else { "" };
        // let properties = self.properties.lock().unwrap();
        // write!(f, "{} {}{}", self.address, properties.local_name.clone()
        //     .unwrap_or_else(|| "(unknown)".to_string()), connected)
        write!(f, "Peripheral")
    }
}

impl Debug for Peripheral {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Peripheral")
            .field("uuid", &self.shared.uuid)
            .field("services", &self.shared.services)
            .field("properties", &self.shared.properties)
            //.field("message_sender", &self.shared.message_sender)
            .finish()
    }
}

#[async_trait]
impl api::Peripheral for Peripheral {
    fn id(&self) -> PeripheralId {
        PeripheralId(self.shared.uuid)
    }

    fn address(&self) -> BDAddr {
        BDAddr::default()
    }

    async fn properties(&self) -> Result<Option<PeripheralProperties>> {
      Ok(Some(self.shared.properties.lock().unwrap().clone()))
  }

    fn services(&self) -> BTreeSet<Service> {
      self.shared.services.lock().unwrap().clone()
    }

    async fn is_connected(&self) -> Result<bool> {
      // Currently the connection simply persists
		  return Ok(true);
    }

    async fn connect(&self) -> Result<()> {
      // Currently the connection simply persists
      Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
      // Currently the connection simply persists
      Ok(())
    }

    async fn discover_services(&self) -> Result<()> {
      /*
      let arr = Array::new();

        let mut options = web_sys::RequestDeviceOptions::new();
        options.accept_all_devices(true);


        //for service_uuid in filter.services {
        //    arr.push(&JsValue::from(service_uuid.to_string()));
        //} 
     
        //options.optional_services(&arr);

        let nav = web_sys::window().unwrap().navigator();
        let p = nav.bluetooth().unwrap().request_device(&options);
        
        let manager = self.shared.manager.clone();
        tokio::spawn(async move {
            match JsFuture::from(p).await {
              Ok(device) => {
                let device = BluetoothDevice::from(device);
                
                if device.name().is_none() {
                  return;
                }
                let name = device.name().unwrap();
                let address = device.id();
    
                log!(format!("Bluetooth device name: {}", name));
  
                let connect_future = device.gatt().unwrap().connect();
                let server: BluetoothRemoteGattServer = match JsFuture::from(connect_future).await {
                  Ok(val) => {
                    log!("CONNECTED");
                    val.into()
                  },
                  Err(_) => {
                    log!("DISCONNECTED");
                    return;
                  }
                };

                if let Some(mut entry) = manager.peripheral_mut(&address.into()) {
                    entry.value_mut().update_properties(args);
                    manager.emit(CentralEvent::DeviceUpdated(address.into()));
                } else {
                    let peripheral = Peripheral::new(Arc::downgrade(&manager), address);
                    peripheral.update_properties(args);
                    manager.add_peripheral(peripheral);
                    manager.emit(CentralEvent::DeviceDiscovered(address.into()));
                }
  
                log!("Trying to get service");
  
                if let Ok(serv) = JsFuture::from(server.get_primary_service_with_str(&service_uuid.to_string())).await 
                {
                    log!(format!(
                      "Service {} found on device {}",
                      service_uuid,
                      device.name().unwrap()
                    ));
                    let service = BluetoothRemoteGattService::from(serv);
  
                    // Gets network info characteristic
                    let chr_uuid = Uuid::from_str("3c9a3f00-8ed3-4bdf-8a39-a01bebede295").unwrap();
                    let char: BluetoothRemoteGattCharacteristic = JsFuture::from(service.get_characteristic_with_str(&chr_uuid.to_string())).await.unwrap().into();
  
                    let mut val = "Yoyo".to_string();
                    let mut bytes = unsafe { val.as_bytes_mut() };
                    let write_future = char.write_value_with_response_with_u8_array(&mut bytes);
  
                    let write_response = JsFuture::from(write_future).await.unwrap();
  
                    log!("Wrote value");
                } else {
                    log!("Failed to get service!");
                }
              }
              Err(e) => {
                log!(&format!(
                  "Error while trying to start bluetooth scan: {:?}",
                  e
                ));
                error!("Error while trying to start bluetooth scan: {:?}", e);
              }
            }
        });
        */
        Ok(())
    }

    async fn write(
        &self,
        characteristic: &Characteristic,
        data: &[u8],
        mut write_type: WriteType,
    ) -> Result<()> {
      let device_id = self.shared.id.clone();
      let service_id = characteristic.service_uuid.clone();
      let characterstic_id = characteristic.uuid.clone();
      let mut data = data.to_vec();
      spawn_local(async move {
		    JsFuture::from(utils::get_bluetooth_characteristic(device_id, service_id, characterstic_id).await.unwrap().write_value_with_response_with_u8_array(&mut data)).await.unwrap();
      });
      Ok(())
    }

    async fn read(&self, characteristic: &Characteristic) -> Result<Vec<u8>> {
      let device_id = self.shared.id.clone();
      let service_id = characteristic.service_uuid.clone();
      let characterstic_id = characteristic.uuid.clone();
      spawn_local(async move {
		    let data = JsFuture::from(utils::get_bluetooth_characteristic(device_id, service_id, characterstic_id).await.unwrap().read_value()).await.unwrap();
      });
      todo!()
    }

    async fn subscribe(&self, characteristic: &Characteristic) -> Result<()> {
      let device_id = self.shared.id.clone();
      let service_id = characteristic.service_uuid.clone();
      let characterstic_id = characteristic.uuid.clone();
      spawn_local(async move {

        let f = Closure::wrap(Box::new(move || {

        }) as Box<dyn FnMut()>);

		    let characteristic = utils::get_bluetooth_characteristic(device_id, service_id, characterstic_id).await.unwrap();
        JsFuture::from(characteristic.start_notifications()).await.unwrap();
        let _ = characteristic.add_event_listener_with_callback("characteristicvaluechanged", f.as_ref().unchecked_ref());
        f.forget();
        
      });
      todo!()
    }

    async fn unsubscribe(&self, characteristic: &Characteristic) -> Result<()> {
      let device_id = self.shared.id.clone();
      let service_id = characteristic.service_uuid.clone();
      let characterstic_id = characteristic.uuid.clone();
      spawn_local(async move {


		    let characteristic = utils::get_bluetooth_characteristic(device_id, service_id, characterstic_id).await.unwrap();
        JsFuture::from(characteristic.stop_notifications()).await.unwrap();
        //let _ = characteristic.remove_event_listener_with_callback("characteristicvaluechanged", f.as_ref().unchecked_ref());
        
      });
      todo!()
    }

    async fn notifications(&self) -> Result<Pin<Box<dyn Stream<Item = ValueNotification> + Send>>> {
      todo!()
      //let receiver = self.shared.notifications_channel.subscribe();
      //Ok(notifications_stream_from_broadcast_receiver(receiver))
    }

    async fn write_descriptor(&self, descriptor: &Descriptor, data: &[u8]) -> Result<()> {
      let device_id = self.shared.id.clone();
      let service_id = descriptor.service_uuid.clone();
      let characterstic_id = descriptor.characteristic_uuid.clone();
      spawn_local(async move {


		    let characteristic = utils::get_bluetooth_characteristic(device_id, service_id, characterstic_id).await.unwrap();
        let descriptors: Array = JsFuture::from(characteristic.get_descriptors()).await.unwrap().into();

        //let _ = characteristic.remove_event_listener_with_callback("characteristicvaluechanged", f.as_ref().unchecked_ref());
        
      });
      todo!()
    }

    async fn read_descriptor(&self, descriptor: &Descriptor) -> Result<Vec<u8>> {
      let device_id = self.shared.id.clone();
      let service_id = descriptor.service_uuid.clone();
      let characterstic_id = descriptor.characteristic_uuid.clone();
      let descriptor_id = descriptor.uuid.clone();
      spawn_local(async move {


		    let characteristic = utils::get_bluetooth_characteristic(device_id, service_id, characterstic_id).await.unwrap();
        let descriptors: Array = JsFuture::from(characteristic.get_descriptors()).await.unwrap().into();
        if let Some(descriptor) = descriptors.iter().map(|x| Into::<BluetoothRemoteGattDescriptor>::into(x)).find(|x| Uuid::from_str(&x.uuid()).unwrap() == descriptor_id) {
          let data_view: DataView = JsFuture::from(descriptor.read_value()).await.unwrap().into();
          data_view.buffer();
        }
        todo!()
        //let _ = characteristic.remove_event_listener_with_callback("characteristicvaluechanged", f.as_ref().unchecked_ref());
        
      });
      todo!()
    }
}

#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_cr")
)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PeripheralId(Uuid);

impl Display for PeripheralId {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl From<Uuid> for PeripheralId {
    fn from(uuid: Uuid) -> Self {
        PeripheralId(uuid)
    }
}

impl From<SendError> for Error {
    fn from(_: SendError) -> Self {
        Error::Other("Channel closed".to_string().into())
    }
}