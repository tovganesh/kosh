use alloc::{collections::BTreeMap, vec::Vec, vec, string::String};
use kosh_types::{DriverId, DriverError};
use crate::driver_loader::DriverBinary;

pub struct DependencyResolver {
    // Maps driver names to their IDs
    name_to_id: BTreeMap<String, DriverId>,
    // Maps driver IDs to their dependents (drivers that depend on them)
    dependents: BTreeMap<DriverId, Vec<DriverId>>,
}

impl DependencyResolver {
    pub fn new() -> Self {
        Self {
            name_to_id: BTreeMap::new(),
            dependents: BTreeMap::new(),
        }
    }

    pub fn resolve_dependencies(&self, binary: &DriverBinary) -> Result<Vec<DriverId>, DriverError> {
        let mut resolved_deps = Vec::new();

        for dep_name in &binary.dependencies {
            let dep_id = self.name_to_id.get(dep_name)
                .ok_or(DriverError::InitializationFailed)?;
            resolved_deps.push(*dep_id);
        }

        // Check for circular dependencies
        self.check_circular_dependencies(&resolved_deps)?;

        Ok(resolved_deps)
    }

    pub fn register_driver_name(&mut self, driver_name: String, driver_id: DriverId) {
        self.name_to_id.insert(driver_name, driver_id);
    }

    pub fn unregister_driver_name(&mut self, driver_name: &str) {
        self.name_to_id.remove(driver_name);
    }

    pub fn add_dependency(&mut self, dependent_id: DriverId, dependency_id: DriverId) {
        self.dependents
            .entry(dependency_id)
            .or_insert_with(Vec::new)
            .push(dependent_id);
    }

    pub fn remove_dependency(&mut self, dependent_id: DriverId, dependency_id: DriverId) {
        if let Some(deps) = self.dependents.get_mut(&dependency_id) {
            deps.retain(|&id| id != dependent_id);
            if deps.is_empty() {
                self.dependents.remove(&dependency_id);
            }
        }
    }

    pub fn has_dependents(&self, driver_id: DriverId) -> bool {
        self.dependents.get(&driver_id)
            .map(|deps| !deps.is_empty())
            .unwrap_or(false)
    }

    pub fn get_dependents(&self, driver_id: DriverId) -> Vec<DriverId> {
        self.dependents.get(&driver_id)
            .cloned()
            .unwrap_or_else(Vec::new)
    }

    pub fn get_load_order(&self, driver_ids: &[DriverId]) -> Result<Vec<DriverId>, DriverError> {
        let mut visited = BTreeMap::new();
        let mut temp_visited = BTreeMap::new();
        let mut result = Vec::new();

        for &driver_id in driver_ids {
            if !visited.contains_key(&driver_id) {
                self.topological_sort(driver_id, &mut visited, &mut temp_visited, &mut result)?;
            }
        }

        Ok(result)
    }

    fn topological_sort(
        &self,
        driver_id: DriverId,
        visited: &mut BTreeMap<DriverId, bool>,
        temp_visited: &mut BTreeMap<DriverId, bool>,
        result: &mut Vec<DriverId>,
    ) -> Result<(), DriverError> {
        if temp_visited.contains_key(&driver_id) {
            return Err(DriverError::InitializationFailed); // Circular dependency
        }

        if visited.contains_key(&driver_id) {
            return Ok(());
        }

        temp_visited.insert(driver_id, true);

        // Visit dependencies first (this would require access to dependency info)
        // For now, we'll assume dependencies are resolved externally

        temp_visited.remove(&driver_id);
        visited.insert(driver_id, true);
        result.push(driver_id);

        Ok(())
    }

    fn check_circular_dependencies(&self, dependencies: &[DriverId]) -> Result<(), DriverError> {
        // Simple cycle detection - in a real implementation, this would be more sophisticated
        let mut visited = BTreeMap::new();
        
        for &dep_id in dependencies {
            if visited.contains_key(&dep_id) {
                return Err(DriverError::InitializationFailed);
            }
            visited.insert(dep_id, true);
        }

        Ok(())
    }

    pub fn can_unload_driver(&self, driver_id: DriverId) -> bool {
        !self.has_dependents(driver_id)
    }

    pub fn get_unload_order(&self, driver_id: DriverId) -> Vec<DriverId> {
        let mut unload_order = Vec::new();
        let mut to_unload = vec![driver_id];

        while let Some(current_id) = to_unload.pop() {
            let dependents = self.get_dependents(current_id);
            for dependent in dependents {
                if !unload_order.contains(&dependent) {
                    to_unload.push(dependent);
                }
            }
            if !unload_order.contains(&current_id) {
                unload_order.push(current_id);
            }
        }

        unload_order.reverse(); // Unload in reverse dependency order
        unload_order
    }
}