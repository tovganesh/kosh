use alloc::{collections::BTreeMap, vec::Vec, string::String};
use kosh_types::{DriverId, ProcessId, DriverError};
use crate::DriverStatus;

#[derive(Debug, Clone)]
pub struct DriverInfo {
    pub driver_id: DriverId,
    pub driver_path: String,
    pub process_id: ProcessId,
    pub dependencies: Vec<DriverId>,
    pub status: DriverStatus,
}

pub struct DriverRegistry {
    drivers: BTreeMap<DriverId, DriverInfo>,
    path_to_id: BTreeMap<String, DriverId>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self {
            drivers: BTreeMap::new(),
            path_to_id: BTreeMap::new(),
        }
    }

    pub fn register_driver(
        &mut self,
        driver_id: DriverId,
        driver_path: &str,
        process_id: ProcessId,
        dependencies: Vec<DriverId>,
    ) -> Result<(), DriverError> {
        // Check if driver is already registered
        if self.drivers.contains_key(&driver_id) {
            return Err(DriverError::ResourceBusy);
        }

        // Check if path is already in use
        if self.path_to_id.contains_key(driver_path) {
            return Err(DriverError::ResourceBusy);
        }

        let driver_info = DriverInfo {
            driver_id,
            driver_path: String::from(driver_path),
            process_id,
            dependencies,
            status: DriverStatus::Loading,
        };

        self.drivers.insert(driver_id, driver_info);
        self.path_to_id.insert(String::from(driver_path), driver_id);

        Ok(())
    }

    pub fn unregister_driver(&mut self, driver_id: DriverId) -> Result<(), DriverError> {
        let driver_info = self.drivers.remove(&driver_id)
            .ok_or(DriverError::InvalidRequest)?;

        self.path_to_id.remove(&driver_info.driver_path);

        Ok(())
    }

    pub fn get_driver_info(&self, driver_id: DriverId) -> Option<&DriverInfo> {
        self.drivers.get(&driver_id)
    }

    pub fn get_driver_by_path(&self, driver_path: &str) -> Option<&DriverInfo> {
        let driver_id = self.path_to_id.get(driver_path)?;
        self.drivers.get(driver_id)
    }

    pub fn update_driver_status(&mut self, driver_id: DriverId, status: DriverStatus) -> Result<(), DriverError> {
        let driver_info = self.drivers.get_mut(&driver_id)
            .ok_or(DriverError::InvalidRequest)?;

        driver_info.status = status;
        Ok(())
    }

    pub fn get_driver_status(&self, driver_id: DriverId) -> Option<DriverStatus> {
        self.drivers.get(&driver_id).map(|info| info.status)
    }

    pub fn list_drivers(&self) -> Vec<DriverId> {
        self.drivers.keys().copied().collect()
    }

    pub fn get_drivers_by_status(&self, status: DriverStatus) -> Vec<DriverId> {
        self.drivers
            .iter()
            .filter(|(_, info)| info.status == status)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn get_driver_dependencies(&self, driver_id: DriverId) -> Option<&Vec<DriverId>> {
        self.drivers.get(&driver_id).map(|info| &info.dependencies)
    }
}