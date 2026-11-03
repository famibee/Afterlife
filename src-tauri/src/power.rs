use std::sync::Mutex;

pub struct PowerGuard {
    guard: Mutex<Option<keepawake::KeepAwake>>,
}

impl PowerGuard {
    pub fn new() -> Self {
        Self {
            guard: Mutex::new(None),
        }
    }

    pub fn enable(&self) {
        let mut guard = self.guard.lock().unwrap();
        if guard.is_some() {
            return;
        }
        match keepawake::Builder::default()
            .display(true)
            .idle(true)
            .sleep(true)
            .reason("Afterlife child active")
            .app_name("Afterlife")
            .app_reverse_domain("com.afterlife.sync")
            .create()
        {
            Ok(kg) => *guard = Some(kg),
            Err(e) => eprintln!("keepawake failed: {e}"),
        }
    }

    pub fn disable(&self) {
        *self.guard.lock().unwrap() = None;
    }
}
