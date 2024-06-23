use std::time::Duration;

use uuid::Uuid;
use web_sys::{Bluetooth, BluetoothDevice, BluetoothRemoteGattCharacteristic};
use futures::channel::oneshot;

pub fn get_bluetooth_api() -> Bluetooth {
	let nav = web_sys::window().unwrap().navigator();
	nav.bluetooth().unwrap()
}

pub fn get_bluetooth_device(id: Uuid) -> BluetoothDevice {
	todo!()
}


pub fn get_bluetooth_server(id: Uuid) -> BluetoothDevice {
	todo!()
}

pub fn get_bluetooth_characteristic(device_id: Uuid, service_id: Uuid, characteristic_id: Uuid) -> BluetoothRemoteGattCharacteristic {
	todo!()
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