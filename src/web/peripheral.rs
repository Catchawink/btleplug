use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    fmt::{self, Debug, Display, Formatter},
    pin::Pin,
    str::FromStr,
    sync::{Arc, Mutex, Weak},
};

use async_trait::async_trait;
use futures::{channel::{mpsc::SendError, oneshot}, Stream};
use gloo_console::log;
use js_sys::{DataView, Uint8Array};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    BluetoothRemoteGattCharacteristic, BluetoothRemoteGattDescriptor,
    BluetoothRemoteGattServer, BluetoothRemoteGattService, DomException,
};

use crate::{
    api::{
        self, BDAddr, CharPropFlags, Characteristic, Descriptor, PeripheralProperties, Service,
        ValueNotification, WriteType,
    },
    common::{adapter_manager::AdapterManager, util::notifications_stream_from_broadcast_receiver},
    Error, Result,
};

use super::{
    tauri,
    utils::{self, is_tauri},
};

/// Browser event listeners must be kept alive for as long as they are registered.
/// Keeping them thread-local is appropriate for wasm/Web Bluetooth and avoids
/// putting `Closure<...>` (browser-only, !Send state) inside `Peripheral::Shared`.
struct NotificationRegistration {
    characteristic: BluetoothRemoteGattCharacteristic,
    listener: Closure<dyn FnMut(JsValue)>,
}

thread_local! {
    static NOTIFICATION_LISTENERS: RefCell<HashMap<(String, Uuid), NotificationRegistration>> =
        RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub struct Peripheral {
    shared: Arc<Shared>,
}

impl Peripheral {
    pub(crate) fn new(
        manager: Weak<AdapterManager<Self>>,
        uuid: Uuid,
        id: String,
        name: Option<String>,
        _services: Vec<Uuid>,
    ) -> Self {
        let properties = Mutex::new(PeripheralProperties {
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

        let (notifications_channel, _) = broadcast::channel(64);

        Self {
            shared: Arc::new(Shared {
                notifications_channel,
                manager,
                uuid,
                id,
                services: Default::default(),
                properties,
            }),
        }
    }

    pub(crate) async fn update_properties(&self) {
        let Some(device) = utils::get_bluetooth_device(self.shared.id.clone()).await else {
            log!("Device not found while updating properties");
            return;
        };

        // `update_properties` should not establish a GATT connection. On Web Bluetooth,
        // requesting/selecting a device and connecting to its GATT server are separate
        // operations. Actual connection is handled by `connect`, service discovery, or
        // characteristic access when needed.
        if let Some(name) = device.name() {
            self.shared.properties.lock().unwrap().local_name = Some(name);
        }
    }

    async fn web_characteristic(
        &self,
        characteristic: &Characteristic,
    ) -> Result<BluetoothRemoteGattCharacteristic> {
        utils::get_bluetooth_characteristic(
            self.shared.id.clone(),
            characteristic.service_uuid,
            characteristic.uuid,
        )
        .await
        .ok_or_else(|| {
            Error::RuntimeError(format!(
                "Bluetooth characteristic {} was not found in service {}",
                characteristic.uuid, characteristic.service_uuid
            ))
        })
    }

    async fn web_descriptor(&self, descriptor: &Descriptor) -> Result<BluetoothRemoteGattDescriptor> {
        let characteristic = utils::get_bluetooth_characteristic(
            self.shared.id.clone(),
            descriptor.service_uuid,
            descriptor.characteristic_uuid,
        )
        .await
        .ok_or_else(|| {
            Error::RuntimeError(format!(
                "Bluetooth characteristic {} was not found while looking up descriptor {}",
                descriptor.characteristic_uuid, descriptor.uuid
            ))
        })?;

        let descriptors = JsFuture::from(characteristic.get_descriptors())
            .await
            .map_err(|error| {
                Error::RuntimeError(format!(
                    "Failed to fetch descriptors for characteristic {}: {:?}",
                    descriptor.characteristic_uuid, error
                ))
            })?;

        descriptors
            .iter()
            .map(Into::<BluetoothRemoteGattDescriptor>::into)
            .find(|candidate| {
                Uuid::from_str(&candidate.uuid())
                    .map(|uuid| uuid == descriptor.uuid)
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                Error::RuntimeError(format!(
                    "Bluetooth descriptor {} was not found",
                    descriptor.uuid
                ))
            })
    }
}

struct Shared {
    notifications_channel: broadcast::Sender<ValueNotification>,
    #[allow(dead_code)]
    manager: Weak<AdapterManager<Peripheral>>,
    uuid: Uuid,
    id: String,
    services: Mutex<BTreeSet<Service>>,
    properties: Mutex<PeripheralProperties>,
}

impl Display for Peripheral {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Peripheral")
    }
}

impl Debug for Peripheral {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Peripheral")
            .field("uuid", &self.shared.uuid)
            .field("services", &self.shared.services)
            .field("properties", &self.shared.properties)
            .finish()
    }
}

#[async_trait(?Send)]
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
        // The Tauri backend does not use the browser's BluetoothDevice registry.
        // Preserve its existing semantics until the Tauri bridge exposes connection state.
        if is_tauri() {
            return Ok(true);
        }

        let device = utils::get_bluetooth_device(self.shared.id.clone())
            .await
            .ok_or(Error::DeviceNotFound)?;

        Ok(device.gatt().map(|gatt| gatt.connected()).unwrap_or(false))
    }

    async fn connect(&self) -> Result<()> {
        if is_tauri() {
            let address = self.address().to_string();
            let (tx, rx) = oneshot::channel::<Vec<crate::models::Service>>();

            spawn_local(async move {
                let services = tauri::connect::<fn()>(address, None)
                    .await
                    .expect("Failed to connect to BLE device!");
                let _ = tx.send(services);
            });

            let discovered_services = rx.await.map_err(|_| {
                Error::RuntimeError("Tauri Bluetooth connect task was cancelled".to_string())
            })?;

            let mut services = self.shared.services.lock().unwrap();
            for service in discovered_services {
                services.insert(service.into());
            }

            return Ok(());
        }

        let device = utils::get_bluetooth_device(self.shared.id.clone())
            .await
            .ok_or(Error::DeviceNotFound)?;
        let gatt = device.gatt().ok_or_else(|| {
            Error::NotSupported("Bluetooth device GATT server is unavailable".to_string())
        })?;

        if gatt.connected() {
            return Ok(());
        }

        match JsFuture::from(gatt.connect()).await {
            Ok(_) => Ok(()),
            Err(error) => {
                let exception: DomException = error.into();

                if gatt.connected() {
                    return Ok(());
                }

                Err(Error::RuntimeError(format!(
                    "Failed to connect: {:?}",
                    exception.name()
                )))
            }
        }
    }

    async fn disconnect(&self) -> Result<()> {
        if is_tauri() {
            // The provided Tauri bridge code does not expose a disconnect operation here.
            return Err(Error::NotSupported(
                "Disconnect is not implemented by the Tauri Bluetooth bridge".to_string(),
            ));
        }

        // Remove all browser callbacks for this device before disconnecting so no stale
        // registrations survive a reconnect.
        let registrations = NOTIFICATION_LISTENERS.with(|listeners| {
            let mut listeners = listeners.borrow_mut();
            let keys = listeners
                .keys()
                .filter(|(device_id, _)| device_id == &self.shared.id)
                .cloned()
                .collect::<Vec<_>>();

            keys.into_iter()
                .filter_map(|key| listeners.remove(&key))
                .collect::<Vec<_>>()
        });

        for registration in registrations {
            let _ = registration.characteristic.remove_event_listener_with_callback(
                "characteristicvaluechanged",
                registration.listener.as_ref().unchecked_ref(),
            );
        }

        let device = utils::get_bluetooth_device(self.shared.id.clone())
            .await
            .ok_or(Error::DeviceNotFound)?;

        if let Some(gatt) = device.gatt() {
            gatt.disconnect();
        }

        Ok(())
    }

    async fn discover_services(&self) -> Result<()> {
        let device = utils::get_bluetooth_device(self.shared.id.clone())
            .await
            .ok_or(Error::DeviceNotFound)?;
        let gatt = device.gatt().ok_or_else(|| {
            Error::NotSupported("Bluetooth device GATT server is unavailable".to_string())
        })?;

        if !gatt.connected() {
            match JsFuture::from(gatt.connect()).await {
                Ok(_) => {}
                Err(error) => {
                    let exception: DomException = error.into();
                    if !gatt.connected() {
                        return Err(Error::RuntimeError(format!(
                            "Failed to connect before service discovery: {:?}",
                            exception.name()
                        )));
                    }
                }
            }
        }

        let server: BluetoothRemoteGattServer = gatt;
        let services = JsFuture::from(server.get_primary_services())
            .await
            .map_err(|error| {
                Error::RuntimeError(format!(
                    "Failed to fetch primary services: {:?}",
                    error
                ))
            })?;

        let mut discovered = BTreeSet::new();

        for service_value in services {
            let service: BluetoothRemoteGattService = service_value.into();
            let service_uuid = Uuid::from_str(&service.uuid()).map_err(Error::from)?;
            let characteristics = JsFuture::from(service.get_characteristics())
                .await
                .map_err(|error| {
                    Error::RuntimeError(format!(
                        "Failed to fetch characteristics for {:?}: {:?}",
                        service_uuid, error
                    ))
                })?;

            let mut found_characteristics = BTreeSet::new();

            for characteristic_value in characteristics {
                let characteristic: BluetoothRemoteGattCharacteristic = characteristic_value.into();
                let uuid = Uuid::from_str(&characteristic.uuid()).map_err(Error::from)?;
                let properties = characteristic.properties();

                let mut descriptor_set = BTreeSet::new();
                let descriptor_values = match JsFuture::from(characteristic.get_descriptors()).await {
                    Ok(values) => values
                        .into_iter()
                        .map(JsValue::from)
                        .collect::<Vec<_>>(),
                    // Some devices/browsers reject descriptor enumeration. That should not
                    // prevent the characteristic itself from being discovered.
                    Err(_) => Vec::new(),
                };

                for descriptor_value in descriptor_values {
                    let descriptor: BluetoothRemoteGattDescriptor = descriptor_value.into();
                    let descriptor_uuid = Uuid::from_str(&descriptor.uuid()).map_err(Error::from)?;
                    descriptor_set.insert(Descriptor {
                        uuid: descriptor_uuid,
                        service_uuid,
                        characteristic_uuid: uuid,
                    });
                }

                let mut char_flags = CharPropFlags::empty();
                if properties.broadcast() {
                    char_flags.insert(CharPropFlags::BROADCAST);
                }
                if properties.read() {
                    char_flags.insert(CharPropFlags::READ);
                }
                if properties.write_without_response() {
                    char_flags.insert(CharPropFlags::WRITE_WITHOUT_RESPONSE);
                }
                if properties.write() {
                    char_flags.insert(CharPropFlags::WRITE);
                }
                if properties.notify() {
                    char_flags.insert(CharPropFlags::NOTIFY);
                }
                if properties.indicate() {
                    char_flags.insert(CharPropFlags::INDICATE);
                }
                if properties.authenticated_signed_writes() {
                    char_flags.insert(CharPropFlags::AUTHENTICATED_SIGNED_WRITES);
                }

                found_characteristics.insert(Characteristic {
                    uuid,
                    service_uuid,
                    properties: char_flags,
                    descriptors: descriptor_set,
                });
            }

            discovered.insert(Service {
                uuid: service_uuid,
                primary: service.is_primary(),
                characteristics: found_characteristics,
            });
        }

        let service_uuids = discovered
            .iter()
            .map(|service| service.uuid)
            .collect::<Vec<_>>();

        *self.shared.services.lock().unwrap() = discovered;
        self.shared.properties.lock().unwrap().services = service_uuids;

        Ok(())
    }

    async fn write(
        &self,
        characteristic: &Characteristic,
        data: &[u8],
        write_type: WriteType,
    ) -> Result<()> {
        let web_characteristic = self.web_characteristic(characteristic).await?;
        let bytes = Uint8Array::from(data);

        let promise = match write_type {
            WriteType::WithResponse => web_characteristic
                .write_value_with_response_with_u8_array(&bytes)
                .map_err(|error| {
                    Error::RuntimeError(format!(
                        "BLE write-with-response could not start for {}: {:?}",
                        characteristic.uuid, error
                    ))
                })?,
            WriteType::WithoutResponse => web_characteristic
                .write_value_without_response_with_u8_array(&bytes)
                .map_err(|error| {
                    Error::RuntimeError(format!(
                        "BLE write-without-response could not start for {}: {:?}",
                        characteristic.uuid, error
                    ))
                })?,
        };

        JsFuture::from(promise).await.map_err(|error| {
            Error::RuntimeError(format!(
                "BLE write failed for characteristic {}: {:?}",
                characteristic.uuid, error
            ))
        })?;

        Ok(())
    }

    async fn read(&self, characteristic: &Characteristic) -> Result<Vec<u8>> {
        let web_characteristic = self.web_characteristic(characteristic).await?;
        let value: DataView = JsFuture::from(web_characteristic.read_value())
            .await
            .map_err(|error| {
                Error::RuntimeError(format!(
                    "BLE read failed for characteristic {}: {:?}",
                    characteristic.uuid, error
                ))
            })?
            .into();

        Ok(data_view_to_vec(&value))
    }

    async fn subscribe(&self, characteristic: &Characteristic) -> Result<()> {
        if !characteristic.properties.contains(CharPropFlags::NOTIFY)
            && !characteristic.properties.contains(CharPropFlags::INDICATE)
        {
            return Err(Error::NotSupported(format!(
                "Characteristic {} does not support notifications/indications",
                characteristic.uuid
            )));
        }

        let key = (self.shared.id.clone(), characteristic.uuid);
        let already_registered =
            NOTIFICATION_LISTENERS.with(|listeners| listeners.borrow().contains_key(&key));

        // Make subscribe idempotent and, importantly, don't stack duplicate callbacks.
        if already_registered {
            return Ok(());
        }

        let web_characteristic = self.web_characteristic(characteristic).await?;
        let notifications = self.shared.notifications_channel.clone();
        let uuid = characteristic.uuid;

        let listener = Closure::wrap(Box::new(move |event: JsValue| {
            let target = js_sys::Reflect::get(&event, &JsValue::from_str("target"))
                .unwrap_or(JsValue::UNDEFINED);
            let Ok(target) = target.dyn_into::<BluetoothRemoteGattCharacteristic>() else {
                return;
            };
            let Some(value) = target.value() else {
                return;
            };

            let bytes = data_view_to_vec(&value);
            let _ = notifications.send(ValueNotification { uuid, value: bytes });
        }) as Box<dyn FnMut(JsValue)>);

        web_characteristic
            .add_event_listener_with_callback(
                "characteristicvaluechanged",
                listener.as_ref().unchecked_ref(),
            )
            .map_err(|error| {
                Error::RuntimeError(format!(
                    "Failed to register notification listener for {}: {:?}",
                    characteristic.uuid, error
                ))
            })?;

        // Register the callback before enabling notifications so the first event cannot
        // arrive before Rust has a listener installed.
        if let Err(error) = JsFuture::from(web_characteristic.start_notifications()).await {
            let _ = web_characteristic.remove_event_listener_with_callback(
                "characteristicvaluechanged",
                listener.as_ref().unchecked_ref(),
            );

            return Err(Error::RuntimeError(format!(
                "BLE subscribe failed for characteristic {}: {:?}",
                characteristic.uuid, error
            )));
        }

        NOTIFICATION_LISTENERS.with(|listeners| {
            listeners.borrow_mut().insert(
                key,
                NotificationRegistration {
                    characteristic: web_characteristic,
                    listener,
                },
            );
        });

        Ok(())
    }

    async fn unsubscribe(&self, characteristic: &Characteristic) -> Result<()> {
        let key = (self.shared.id.clone(), characteristic.uuid);
        let registration =
            NOTIFICATION_LISTENERS.with(|listeners| listeners.borrow_mut().remove(&key));

        // No callback is registered, so this is already unsubscribed from our API's
        // point of view. Keeping this idempotent avoids unnecessary browser errors.
        let Some(registration) = registration else {
            return Ok(());
        };

        let stop_result = JsFuture::from(registration.characteristic.stop_notifications()).await;

        let remove_result = registration.characteristic.remove_event_listener_with_callback(
            "characteristicvaluechanged",
            registration.listener.as_ref().unchecked_ref(),
        );

        if let Err(error) = stop_result {
            return Err(Error::RuntimeError(format!(
                "BLE unsubscribe failed for characteristic {}: {:?}",
                characteristic.uuid, error
            )));
        }

        remove_result.map_err(|error| {
            Error::RuntimeError(format!(
                "Failed to remove notification listener for {}: {:?}",
                characteristic.uuid, error
            ))
        })?;

        Ok(())
    }

    async fn notifications(&self) -> Result<Pin<Box<dyn Stream<Item = ValueNotification> + Send>>> {
        let receiver = self.shared.notifications_channel.subscribe();
        Ok(notifications_stream_from_broadcast_receiver(receiver))
    }

    async fn write_descriptor(&self, descriptor: &Descriptor, data: &[u8]) -> Result<()> {
        let web_descriptor = self.web_descriptor(descriptor).await?;
        let bytes = Uint8Array::from(data);
        let promise = web_descriptor
            .write_value_with_u8_array(&bytes)
            .map_err(|error| {
                Error::RuntimeError(format!(
                    "BLE descriptor write could not start for {}: {:?}",
                    descriptor.uuid, error
                ))
            })?;

        JsFuture::from(promise).await.map_err(|error| {
            Error::RuntimeError(format!(
                "BLE descriptor write failed for {}: {:?}",
                descriptor.uuid, error
            ))
        })?;

        Ok(())
    }

    async fn read_descriptor(&self, descriptor: &Descriptor) -> Result<Vec<u8>> {
        let web_descriptor = self.web_descriptor(descriptor).await?;
        let value: DataView = JsFuture::from(web_descriptor.read_value())
            .await
            .map_err(|error| {
                Error::RuntimeError(format!(
                    "BLE descriptor read failed for {}: {:?}",
                    descriptor.uuid, error
                ))
            })?
            .into();

        Ok(data_view_to_vec(&value))
    }
}

fn data_view_to_vec(value: &DataView) -> Vec<u8> {
    let bytes = Uint8Array::new(&value.buffer());

    let start = value.byte_offset() as u32;
    let end = start + value.byte_length() as u32;

    bytes.subarray(start, end).to_vec()
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
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
