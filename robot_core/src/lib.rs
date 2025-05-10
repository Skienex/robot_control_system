use log::{error, info, log, warn};
use pca9685_rppal::Pca9685;
use robot_web::CommandPayload;
use rppal::gpio::Gpio;
use std::sync::mpsc;
use std::thread;

const FREQ: f32 = 200.0; // 50 Hz

pub fn main() {
    env_logger::init();

    let (tx, rx) = mpsc::channel::<CommandPayload>();

    let server_tx_clone = tx.clone();
    let server_thread_handle = thread::spawn(move || {
        let host = "0.0.0.0".to_string();
        let port = 8080;
        robot_web::start_axum_server_in_thread(host, port, server_tx_clone);
    });

    info!("Webserver wird in einem separaten Thread gestartet. Hauptthread lauscht auf Befehle...");

    let controller_res = Pca9685::new();
    if controller_res.is_err() {
        // FICK DICH INS KNIE
        error!("[Main Thread] Motor Controller not initialized (perhaps not plugged in?!)")
    }

    let mut turbo = false;

    let mut controller = controller_res.unwrap();
    controller.init().unwrap();

    controller.set_pwm_freq(FREQ).unwrap();

    let gpio = Gpio::new().unwrap();
    let mut horn = gpio.get(23).unwrap().into_output();
    let mut lights = gpio.get(24).unwrap().into_output();

    loop {
        match rx.recv() {
            Ok(command_payload) => {
                info!(
                    "[Main Thread] Befehl vom Server empfangen: {:?}",
                    command_payload
                );
                match command_payload.command.as_str() {
                    "speed" => {
                        if let Some(s) = command_payload.value.as_i64() {
                            info!("[Main Thread] Successfully received speed value");
                            controller.set_pwm(0, 0, speed_to_pulse(s, turbo)).unwrap()
                        } else {
                            info!("[Main Thread] No speed value provided")
                        }
                    }
                    "direction" => {
                        if let Some(d) = command_payload.value.as_i64() {
                            info!("[Main Thread] Successfully received direction value");
                            controller.set_pwm(0, 0, direction_to_pulse(d)).unwrap();
                        } else {
                            info!("[Main Thread] No direction value provided")
                        }
                    }
                    "headlights" => {
                        if let Some(h) = command_payload.value.as_bool() {
                            info!("[Main Thread] Successfully received headlights value");
                            if h {
                                lights.set_high();
                            } else {
                                lights.set_low();
                            }
                        } else {
                            info!("[Main Thread] No headlights value provided")
                        }
                    }
                    "horn" => {
                        if let Some(h) = command_payload.value.as_bool() {
                            info!("[Main Thread] Successfully received horn value");
                            if h {
                                horn.set_high();
                            } else {
                                horn.set_low();
                            }
                        } else {
                            info!("[Main Thread] No horn value provided")
                        }
                    }
                    "turbo" => {
                        if let Some(t) = command_payload.value.as_bool() {
                            info!("[Main Thread] Successfully received turbo value");
                            turbo = t;
                        } else {
                            info!("[Main Thread] No turbo value provided")
                        }
                    }
                    "calibrate" => {}
                    _ => {}
                }
            }
            Err(e) => {
                error!(
                    "[Main Thread] Fehler beim Empfangen vom Kanal (Server vermutlich beendet): {}",
                    e
                );
                break;
            }
        }
    }

    warn!("[Main Thread] Empfangs-Loop beendet. Warte auf Beendigung des Server-Threads...");
    if server_thread_handle.join().is_err() {
        error!("[Main Thread] Server-Thread konnte nicht sauber beendet werden (panic).");
    }

    info!("[Main Thread] Anwendung wird beendet.");
}

fn speed_to_pulse(speed: i64, turbo: bool) -> u16 {
    // Clamp auf erlaubten Bereich
    let x = speed.clamp(-100, 100);

    if (-7..=7).contains(&x) {
        1450
    } else if x < -7 {
        let slope = 200.0 / 93.0;
        let y = 1450.0 + (x as f32 + 7.0) * slope;
        if turbo {
            return y.round() as u16 - 100;
        }
        y.round() as u16
    } else {
        let slope = 750.0 / 93.0;
        let y = 1450.0 + (x as f32 - 7.0) * slope;
        if turbo {
            return y.round() as u16 + 100;
        }
        y.round() as u16
    }
}

fn direction_to_pulse(direction: i64) -> u16 {
    let _ = direction.clamp(-100, 100);
    todo!()
}
