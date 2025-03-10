use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use tauri_sys::core::{invoke, Channel};
use crate::models::*;
use serde_json::json;

/// Connect to this BLE device.
///
/// The device’s address (from `device.address`) is used.
/// Optionally, a disconnect callback can be provided.
pub async fn connect<F>(address: String, on_disconnect: Option<F>) -> Result<Vec<Service>, JsValue>
where
    F: FnMut() + 'static,
{
    let disconnect_channel: Channel<()> = Channel::new();
    let args = json!({
        "address": address,
        "onDisconnect": disconnect_channel,
    });
    if let Some(mut callback) = on_disconnect {
        // Spawn a task that calls the disconnect callback when a message is received.
        let mut chan = disconnect_channel;
        spawn_local(async move {
            use futures::StreamExt;
            while let Some(()) = chan.next().await {
                callback();
            }
        });
    }
    let services = invoke::<Vec<Service>>("plugin:blec|connect", &args).await;
    Ok(services)
}

/// Write raw data to a BLE characteristic.
pub async fn ble_device_send(
    device: &BleDevice,
    characteristic: String,
    data: Vec<u8>,
    write_type: Option<&str>,
) -> Result<(), JsValue> {
    let write_type = write_type.unwrap_or("withResponse");
    let args = json!({
        "characteristic": characteristic,
        "data": data,
        "writeType": write_type,
    });
    invoke::<()>("plugin:blec|send", &args).await;
    Ok(())
}

/// Write a string to a BLE characteristic.
pub async fn ble_device_send_string(
    device: &BleDevice,
    characteristic: String,
    data: String,
    write_type: Option<&str>,
) -> Result<(), JsValue> {
    let write_type = write_type.unwrap_or("withResponse");
    let args = json!({
        "characteristic": characteristic,
        "data": data,
        "writeType": write_type,
    });
    invoke::<()>("plugin:blec|send_string", &args).await;
    Ok(())
}

/// Read raw data from a BLE characteristic.
pub async fn ble_device_read(
    device: &BleDevice,
    characteristic: String,
) -> Result<Vec<u8>, JsValue> {
    let args = json!({ "characteristic": characteristic });
    let res = invoke::<Vec<u8>>("plugin:blec|recv", &args).await;
    Ok(res)
}

/// Read a string from a BLE characteristic.
pub async fn ble_device_read_string(
    device: &BleDevice,
    characteristic: String,
) -> Result<String, JsValue> {
    let args = json!({ "characteristic": characteristic });
    let res = invoke::<String>("plugin:blec|recv_string", &args).await;
    Ok(res)
}

/// Subscribe to notifications (raw data) for a BLE characteristic.
pub async fn ble_device_subscribe<F>(
    device: &BleDevice,
    characteristic: String,
    mut handler: F,
) -> Result<(), JsValue>
where
    F: FnMut(Vec<u8>) + 'static,
{
    let mut on_data: Channel<Vec<u8>> = Channel::new();
    let args = json!({
        "characteristic": characteristic,
        "onData": on_data,
    });
    spawn_local(async move {
        use futures::StreamExt;
        while let Some(data) = on_data.next().await {
            handler(data);
        }
    });
    invoke::<()>("plugin:blec|subscribe", &args).await;
    Ok(())
}

/// Subscribe to notifications (string data) for a BLE characteristic.
pub async fn ble_device_subscribe_string<F>(
    device: &BleDevice,
    characteristic: String,
    mut handler: F,
) -> Result<(), JsValue>
where
    F: FnMut(String) + 'static,
{
    let mut on_data: Channel<String> = Channel::new();
    let args = json!({
        "characteristic": characteristic,
        "onData": on_data,
    });
    spawn_local(async move {
        use futures::StreamExt;
        while let Some(data) = on_data.next().await {
            handler(data);
        }
    });
    invoke::<()>("plugin:blec|subscribe_string", &args).await;
    Ok(())
}

/// Unsubscribe from a BLE characteristic.
pub async fn ble_device_unsubscribe(
    device: &BleDevice,
    characteristic: String,
) -> Result<(), JsValue> {
    let args = json!({ "characteristic": characteristic });
    invoke::<()>("plugin:blec|unsubscribe", &args).await;
    Ok(())
}
/// Scan for BLE devices.
///
/// Spawns a task that calls the provided handler for each batch of devices received.
pub async fn start_scan<F>(
    mut handler: F,
    timeout: Option<u64>,
    services: Vec<Uuid>,
) -> Result<(), JsValue>
where
    F: FnMut(Vec<BleDevice>) + 'static,
{
    let timeout = timeout.unwrap_or(10_000);
    let mut on_devices: Channel<Vec<BleDevice>> = Channel::new();
    let args = json!({
        "timeout": timeout,
        "services": services,
        "onDevices": on_devices,
    });
    spawn_local(async move {
        use futures::StreamExt;
        while let Some(devices) = on_devices.next().await {
            handler(devices);
        }
    });
    invoke::<()>("plugin:blec|scan", &args).await;
    Ok(())
}

/// Stop scanning for BLE devices.
pub async fn stop_scan() -> Result<(), JsValue> {
    invoke::<()>("plugin:blec|stop_scan", &json!({})).await;
    Ok(())
}

/// Register a handler for connection state updates.
pub async fn get_connection_updates<F>(
    mut handler: F,
) -> Result<(), JsValue>
where
    F: FnMut(bool) + 'static,
{
    let mut connection_chan: Channel<bool> = Channel::new();
    let args = json!({ "update": connection_chan });
    spawn_local(async move {
        use futures::StreamExt;
        while let Some(connected) = connection_chan.next().await {
            handler(connected);
        }
    });
    invoke::<()>("plugin:blec|connection_state", &args).await;
    Ok(())
}

/// Register a handler for scanning state updates.
pub async fn get_scanning_updates<F>(
    mut handler: F,
) -> Result<(), JsValue>
where
    F: FnMut(bool) + 'static,
{
    let mut scanning_chan: Channel<bool> = Channel::new();
    let args = json!({ "update": scanning_chan });
    spawn_local(async move {
        use futures::StreamExt;
        while let Some(scanning) = scanning_chan.next().await {
            handler(scanning);
        }
    });
    invoke::<()>("plugin:blec|scanning_state", &args).await;
    Ok(())
}

/// Disconnect from the currently connected device.
pub async fn disconnect() -> Result<(), JsValue> {
    invoke::<()>("plugin:blec|disconnect", &json!({})).await;
    Ok(())
}
