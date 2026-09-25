use std::time::Duration;

use gloo_console::log;
use uuid::Uuid;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Bluetooth, BluetoothDevice, BluetoothRemoteGattCharacteristic, BluetoothRemoteGattServer,
    BluetoothRemoteGattService, window,
};

pub fn is_tauri() -> bool {
    if js_sys::Reflect::get(&window().unwrap(), &JsValue::from_str("__TAURI__"))
        .map(|value| value.is_object())
        .unwrap_or(false)
    {
        return true;
    }

    let navigator = window().unwrap().navigator();
    let user_agent = navigator.user_agent().unwrap_or_else(|_| String::new());
    user_agent.contains("Tauri")
}

pub fn get_bluetooth_api() -> Bluetooth {
    let nav = web_sys::window().unwrap().navigator();
    nav.bluetooth().unwrap()
}

pub async fn get_bluetooth_device(device_id: String) -> Option<BluetoothDevice> {
    super::adapter::DEVICES.with_borrow(|devices| devices.get(&device_id).cloned())
}

pub async fn get_bluetooth_device_server(device_id: String) -> Option<BluetoothRemoteGattServer> {
    let device = get_bluetooth_device(device_id).await?;
    let gatt = device.gatt()?;

    if gatt.connected() {
        return Some(gatt);
    }

    log!("BLE GATT server is disconnected; connect explicitly before characteristic access");
    None
}

pub async fn get_bluetooth_characteristic(
    device_id: String,
    service_id: Uuid,
    characteristic_id: Uuid,
) -> Option<BluetoothRemoteGattCharacteristic> {
    let server = get_bluetooth_device_server(device_id).await?;

    let services = match JsFuture::from(server.get_primary_services()).await {
        Ok(value) => value,
        Err(error) => {
            log!(&format!("Error getting Bluetooth services: {:?}", error));
            return None;
        }
    };

    for service_value in services {
        let service: BluetoothRemoteGattService = service_value.into();

        let Ok(uuid) = Uuid::parse_str(&service.uuid()) else {
            continue;
        };

        if uuid != service_id {
            continue;
        }

        let characteristics = match JsFuture::from(service.get_characteristics()).await {
            Ok(value) => value,
            Err(error) => {
                log!(&format!(
                    "Error getting Bluetooth characteristics for service {}: {:?}",
                    service_id, error
                ));
                return None;
            }
        };

        return characteristics
            .iter()
            .map(Into::<BluetoothRemoteGattCharacteristic>::into)
            .find(|characteristic| {
                Uuid::parse_str(&characteristic.uuid())
                    .map(|uuid| uuid == characteristic_id)
                    .unwrap_or(false)
            });
    }

    None
}

pub async fn sleep(duration: Duration) {
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let result = window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                &resolve,
                duration.as_millis().min(i32::MAX as u128) as i32,
            );
        if let Err(error) = result {
            let _ = reject.call1(&JsValue::UNDEFINED, &error);
        }
    });
    let _ = JsFuture::from(promise).await;
}
