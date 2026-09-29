use std::{collections::HashMap, sync::{Arc, Mutex}};
use tokio::sync::watch;

#[derive(Clone, Default)]
pub struct Coordinator {
    inner: Arc<Mutex<Schedule>>,
}
#[derive(Default)]
struct Schedule {
    checking_all: bool,
    active: HashMap<&'static str, watch::Receiver<bool>>,
}
pub struct AppLease {
    coordinator: Coordinator,
    id: &'static str,
    finished: watch::Sender<bool>,
}
pub struct AllLease {
    coordinator: Coordinator,
}
pub enum PendingCheck {
    New(AppLease),
    Existing(watch::Receiver<bool>),
}
pub struct Batch {
    pub lease: AllLease,
    pub checks: Vec<(&'static str, PendingCheck)>,
}

impl Coordinator {
    pub fn snapshot(&self) -> (bool, Vec<String>) {
        let schedule = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut apps: Vec<_> = schedule.active.keys().map(|id| (*id).to_string()).collect();
        apps.sort();
        (schedule.checking_all, apps)
    }
    fn reserve(&self, schedule: &mut Schedule, id: &'static str) -> AppLease {
        let (finished, receiver) = watch::channel(false);
        schedule.active.insert(id, receiver);
        AppLease { coordinator: self.clone(), id, finished }
    }
    pub fn begin_app(&self, id: &'static str) -> Result<AppLease, String> {
        let mut schedule = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if schedule.checking_all {
            return Err("Espera a que termine la verificación de todas las apps".to_string());
        }
        if schedule.active.contains_key(id) {
            return Err("Esta aplicación ya tiene una operación en curso".to_string());
        }
        Ok(self.reserve(&mut schedule, id))
    }
    pub fn begin_all(&self, ids: &[&'static str]) -> Result<Batch, String> {
        let mut schedule = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if schedule.checking_all {
            return Err("La verificación de todas las apps ya está en curso".to_string());
        }
        schedule.checking_all = true;
        let checks = ids.iter().map(|&id| {
            let pending = if let Some(receiver) = schedule.active.get(id) {
                PendingCheck::Existing(receiver.clone())
            } else {
                PendingCheck::New(self.reserve(&mut schedule, id))
            };
            (id, pending)
        }).collect();
        Ok(Batch { lease: AllLease { coordinator: self.clone() }, checks })
    }
}

impl Drop for AppLease {
    fn drop(&mut self) {
        let mut schedule = self.coordinator.inner.lock().unwrap_or_else(|e| e.into_inner());
        schedule.active.remove(self.id);
        self.finished.send_replace(true);
    }
}
impl Drop for AllLease {
    fn drop(&mut self) {
        self.coordinator.inner.lock().unwrap_or_else(|e| e.into_inner()).checking_all = false;
    }
}

