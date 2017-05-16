use std::io;
use std::time::Duration;
use std::thread;

mod device;
mod devicemap;
mod hidraw;
mod monitor;
mod util;

use runloop::RunLoop;

use self::devicemap::DeviceMap;
use self::monitor::Monitor;

pub struct PlatformManager {
    // Handle to the thread loop.
    thread: Option<RunLoop>
}

impl PlatformManager {
    pub fn new() -> Self {
        Self { thread: None }
    }

    pub fn register<F>(&mut self, timeout: u64, challenge: Vec<u8>, application: Vec<u8>, callback: F) -> io::Result<()>
        where F: FnOnce(io::Result<Vec<u8>>), F: Send + 'static
    {
        // Abort any prior register/sign calls.
        self.cancel();

        self.thread = Some(RunLoop::new(move |alive| {
            let mut monitor = Monitor::new()?;
            let mut devices = DeviceMap::new();

            // Helper to stop monitor and call back.
            let complete = |monitor: &mut Monitor, rv| {
                monitor.stop();
                callback(rv);
                Ok(())
            };

            while alive() {
                // Add/remove devices.
                for event in monitor.events() {
                    devices.process_event(event);
                }

                // Try to register each device.
                for device in devices.values_mut() {
                    if let Ok(bytes) = super::u2f_register(device, &challenge, &application) {
                        return complete(&mut monitor, Ok(bytes));
                    }
                }

                // Wait a little before trying again.
                thread::sleep(Duration::from_millis(100));
            }

            complete(&mut monitor, Err(util::io_err("cancelled or timed out")))
        }, timeout)?);

        Ok(())
    }

    pub fn sign<F>(&mut self, timeout: u64, challenge: Vec<u8>, application: Vec<u8>, key_handle: Vec<u8>, callback: F) -> io::Result<()>
        where F: FnOnce(io::Result<Vec<u8>>), F: Send + 'static
    {
        // Abort any prior register/sign calls.
        self.cancel();

        self.thread = Some(RunLoop::new(move |alive| {
            let mut monitor = Monitor::new()?;
            let mut devices = DeviceMap::new();

            // Helper to stop monitor and call back.
            let complete = |monitor: &mut Monitor, rv| {
                monitor.stop();
                callback(rv);
                Ok(())
            };

            while alive() {
                // Add/remove devices.
                for event in monitor.events() {
                    devices.process_event(event);
                }

                // Try signing with each device.
                for device in devices.values_mut() {
                    // Check if they key handle belongs to the current device.
                    let is_valid = match super::u2f_is_keyhandle_valid(device, &challenge, &application, &key_handle) {
                        Ok(valid) => valid,
                        Err(_) => continue
                    };

                    if is_valid {
                        // If yes, try to sign.
                        // TODO: transmit garbage challenge and application
                        if let Ok(bytes) = super::u2f_sign(device, &challenge, &application, &key_handle) {
                            return complete(&mut monitor, Ok(bytes))
                        }
                    } else {
                        // If no, keep registering and blinking.
                        if let Ok(_) = super::u2f_register(device, &challenge, &application) {
                            return complete(&mut monitor, Err(util::io_err("invalid key")))
                        }
                    }
                }

                // Wait a little before trying again.
                thread::sleep(Duration::from_millis(100));
            }

            complete(&mut monitor, Err(util::io_err("cancelled or timed out")))
        }, timeout)?);

        Ok(())
    }

    // This might block.
    pub fn cancel(&mut self) {
        if let Some(mut thread) = self.thread.take() {
            thread.cancel();
        }
    }
}
