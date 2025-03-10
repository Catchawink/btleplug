use std::collections::HashMap;

use crate::api::BDAddr;
use enumflags2::BitFlags;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use uuid::Uuid;

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleDevice {
    pub address: String,
    pub name: String,
    pub is_connected: bool,
    #[serde_as(as = "Vec<(_, _)>")]
    pub manufacturer_data: HashMap<u16, Vec<u8>>,
    pub services: Vec<Uuid>,
}

impl Eq for BleDevice {}

impl PartialOrd for BleDevice {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BleDevice {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.address.cmp(&other.address)
    }
}

impl PartialEq for BleDevice {
    fn eq(&self, other: &Self) -> bool {
        self.address == other.address
    }
}

impl BleDevice {
    pub async fn from_peripheral<P: crate::api::Peripheral>(
        peripheral: &P,
    ) -> crate::Result<Self> {
        #[cfg(target_vendor = "apple")]
        let address = peripheral.id().to_string();
        #[cfg(not(target_vendor = "apple"))]
        let address = peripheral.address().to_string();
        let properties = peripheral.properties().await?.unwrap_or_default();
        let name = properties
            .local_name
            .unwrap_or_else(|| peripheral.id().to_string());
        Ok(Self {
            address,
            name,
            manufacturer_data: properties.manufacturer_data,
            services: properties.services,
            is_connected: peripheral.is_connected().await?,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Service {
    pub uuid: Uuid,
    pub characteristics: Vec<Characteristic>,
}

impl From<crate::api::Service> for Service {
    fn from(service: crate::api::Service) -> Self {
        Self {
            uuid: service.uuid,
            characteristics: service
                .characteristics
                .iter().cloned()
                .map(Characteristic::from)
                .collect(),
        }
    }
}

impl Into<crate::api::Service> for Service {
	fn into(self) -> crate::api::Service {
		crate::api::Service {
			uuid: self.uuid,
			primary: true,
			characteristics: self
				.characteristics
				.iter().cloned()
				.map(Characteristic::into)
				.collect(),
		}
	}
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Characteristic {
    pub uuid: Uuid,
	pub service_uuid: Uuid,
    pub descriptors: Vec<Uuid>,
    pub properties: BitFlags<CharProps>,
}

impl From<crate::api::Characteristic> for Characteristic {
    fn from(characteristic: crate::api::Characteristic) -> Self {
        Self {
            uuid: characteristic.uuid,
			service_uuid: characteristic.service_uuid,
            descriptors: characteristic.descriptors.iter().map(|d| d.uuid).collect(),
            properties: from_flags(characteristic.properties),
        }
    }
}

impl Into<crate::api::Characteristic> for Characteristic {
	fn into(self) -> crate::api::Characteristic {
		crate::api::Characteristic {
			uuid: self.uuid,
			service_uuid: self.service_uuid,
			properties: todo!(),
			descriptors: todo!(),
		}
	}
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[enumflags2::bitflags]
#[repr(u8)]
pub enum CharProps {
    Broadcast,
    Read,
    WriteWithoutResponse,
    Write,
    Notify,
    Indicate,
    AuthenticatedSignedWrites,
    ExtendedProperties,
}

impl From<crate::api::CharPropFlags> for CharProps {
    fn from(flag: crate::api::CharPropFlags) -> Self {
        match flag {
            crate::api::CharPropFlags::BROADCAST => CharProps::Broadcast,
            crate::api::CharPropFlags::READ => CharProps::Read,
            crate::api::CharPropFlags::WRITE_WITHOUT_RESPONSE => CharProps::WriteWithoutResponse,
            crate::api::CharPropFlags::WRITE => CharProps::Write,
            crate::api::CharPropFlags::NOTIFY => CharProps::Notify,
            crate::api::CharPropFlags::INDICATE => CharProps::Indicate,
            crate::api::CharPropFlags::AUTHENTICATED_SIGNED_WRITES => {
                CharProps::AuthenticatedSignedWrites
            }
            crate::api::CharPropFlags::EXTENDED_PROPERTIES => CharProps::ExtendedProperties,
            _ => unreachable!(),
        }
    }
}

impl Into<crate::api::CharPropFlags> for CharProps {
    fn into(self) -> crate::api::CharPropFlags {
        match self {
            CharProps::Broadcast => crate::api::CharPropFlags::BROADCAST,
            CharProps::Read => crate::api::CharPropFlags::READ,
            CharProps::WriteWithoutResponse => crate::api::CharPropFlags::WRITE_WITHOUT_RESPONSE,
            CharProps::Write => crate::api::CharPropFlags::WRITE,
            CharProps::Notify => crate::api::CharPropFlags::NOTIFY,
            CharProps::Indicate => crate::api::CharPropFlags::INDICATE,
            CharProps::AuthenticatedSignedWrites => crate::api::CharPropFlags::AUTHENTICATED_SIGNED_WRITES,
           	CharProps::ExtendedProperties => crate::api::CharPropFlags::EXTENDED_PROPERTIES,
            _ => unreachable!(),
        }
    }
}

fn from_flags(properties: crate::api::CharPropFlags) -> BitFlags<CharProps, u8> {
    let mut flags = BitFlags::empty();
    for flag in properties.iter() {
        flags |= CharProps::from(flag);
    }
    flags
}

fn into_flags(properties: BitFlags<CharProps, u8>) -> crate::api::CharPropFlags {
    let mut flags = crate::api::CharPropFlags::empty();
    for flag in properties.iter() {
        flags |= flag.into();
    }
    flags
}

#[must_use]
pub fn fmt_addr(addr: BDAddr) -> String {
    let a = addr.into_inner();
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        a[0], a[1], a[2], a[3], a[4], a[5]
    )
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteType {
    /// aka request.
    WithResponse,
    /// aka command.
    WithoutResponse,
}

impl From<WriteType> for crate::api::WriteType {
    fn from(write_type: WriteType) -> Self {
        match write_type {
            WriteType::WithResponse => crate::api::WriteType::WithResponse,
            WriteType::WithoutResponse => crate::api::WriteType::WithoutResponse,
        }
    }
}

/// Filter for discovering devices.
/// Only devices matching the filter will be returned by the `handler::discover` method
pub enum ScanFilter {
    None,
    /// Matches if the device advertises the specified service.
    Service(Uuid),
    /// Matches if the device advertises any of the specified services.
    AnyService(Vec<Uuid>),
    /// Matches if the device advertises all of the specified services.
    AllServices(Vec<Uuid>),
    /// Matches if the device advertises the specified manufacturer data.
    ManufacturerData(u16, Vec<u8>),
}
