use std::time::Duration;

use js_sys::Array;
use uuid::Uuid;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Bluetooth, BluetoothDevice, BluetoothRemoteGattCharacteristic, BluetoothRemoteGattServer, BluetoothRemoteGattService};
use futures::channel::oneshot;
use gloo_console::log;
use crate::Error;

pub fn get_bluetooth_api() -> Bluetooth {
	let nav = web_sys::window().unwrap().navigator();
	nav.bluetooth().unwrap()
}

pub async fn get_bluetooth_device(device_id: String) -> Option<BluetoothDevice> {
    super::adapter::DEVICES.with_borrow_mut(|devices| {
      let device = devices.get_mut(&device_id);
      device.cloned()
    })
}

pub async fn get_bluetooth_device_server(device_id: String) -> Option<BluetoothRemoteGattServer> {
    let connect_future = get_bluetooth_device(device_id).await?.gatt().unwrap().connect();
    log!("Connecting to device...");

    let server: BluetoothRemoteGattServer = match JsFuture::from(connect_future).await {
      Ok(val) => {
        val.into()
      },
      Err(_) => {
        return None;
      }
    };
    Some(server)
}

pub async fn get_bluetooth_characteristic(device_id: String, service_id: Uuid, characteristic_id: Uuid) -> Option<BluetoothRemoteGattCharacteristic> {
  let server = get_bluetooth_device_server(device_id).await?;

  let _services: Array = match JsFuture::from(server.get_primary_services()).await {
      Ok(val) => {
        val.into()
      },
      Err(e) => {
        log!(&format!("Error getting bluetooth services: {:?}", e));
        return None;
      },
  };
  
  for _service in _services {
    let _service: BluetoothRemoteGattService = _service.into();

    if Uuid::parse_str(&_service.uuid()).unwrap() != service_id {
      continue;
    }

    let _characteristics: Array = match JsFuture::from(_service.get_characteristics()).await {
      Ok(val) => {
        val.into()
      },
      Err(e) => {
        log!(&format!("Error getting bluetooth characteristics: {:?}", e));
        return None;
      },
    };

    let mut _characteristics = _characteristics.iter().map(|x| Into::<BluetoothRemoteGattCharacteristic>::into(x));
    let characteristic = _characteristics.find(|x| Uuid::parse_str(&x.uuid()).unwrap() == characteristic_id);
    return characteristic;
  }
  None
}

pub async fn sleep(duration: Duration) {
    let (response_tx, response_rx) = oneshot::channel::<()>();
    
   wasm_bindgen_futures::spawn_local(async move {
        /*
            let mut cb = |resolve: js_sys::Function, _reject: js_sys::Function| {
                web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 10)
                    .unwrap();
            };
        
            let p = js_sys::Promise::new(&mut cb);
            wasm_bindgen_futures::JsFuture::from(p).await.unwrap();
 */
            async_std::task::sleep(std::time::Duration::from_millis(duration.as_millis() as u64)).await;
                
            response_tx.send(()).unwrap();
        }
    );
    response_rx.await.unwrap();
}