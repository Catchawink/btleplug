use uuid::Uuid;
use web_sys::{Bluetooth, BluetoothDevice, BluetoothRemoteGattCharacteristic};

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