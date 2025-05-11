use log::{error, info, warn};
use pca9685_rppal::Pca9685;
use robot_web::CommandPayload;
use rppal::gpio::Gpio;
use std::sync::mpsc;
use std::thread;

const FREQ: f32 = 200.0; // 50 Hz

const FRONT_LEFT_PULSE: u16 = 1150;
const FRONT_RIGHT_PULSE: u16 = 305;

const BACK_LEFT_PULSE: u16 = 1375;
const BACK_RIGHT_PULSE: u16 = 2185;
const MOTOR_CHANNEL: u8 = 0;
const FRONT_STEERING_CHANNEL: u8 = 1;
const BACK_STEERING_CHANNEL: u8 = 2;

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
        error!("[Main Thread] Motor Controller not initialized (perhaps not plugged in?!)");
        panic!("[Main Thread] Motor Controller init failed.");
    }

    let mut turbo = false;

    let mut controller = controller_res.unwrap();
    controller.init().unwrap();

    if let Err(e) = controller.set_pwm_freq(FREQ) {
        error!("[Main Thread] Failed to set PWM frequency: {:?}", e);
        panic!("[Main Thread] PWM frequency setting failed.");
    }

    let gpio_res = Gpio::new();
    let (mut horn, mut lights) = if let Ok(gpio) = gpio_res {
        let horn_pin = gpio.get(23).map(|p| p.into_output());
        let lights_pin = gpio.get(24).map(|p| p.into_output());

        if horn_pin.is_err() || lights_pin.is_err() {
            error!("[Main Thread] Failed to initialize GPIO pins.");
            (None, None)
        } else {
            (horn_pin.ok(), lights_pin.ok())
        }
    } else {
        error!("[Main Thread] Failed to initialize GPIO.");
        (None, None)
    };

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
                            info!("[Main Thread] Successfully received speed value: {}", s);
                            let pulse = speed_to_pulse(s, turbo);
                            info!(
                                "[Main Thread] Setting motor (channel {}) pulse to: {}",
                                MOTOR_CHANNEL, pulse
                            );
                            if let Err(e) = controller.set_pwm(MOTOR_CHANNEL, 0, pulse) {
                                error!("[Main Thread] Failed to set motor PWM: {:?}", e);
                            }
                        } else {
                            warn!("[Main Thread] No speed value provided");
                        }
                    }
                    "direction" => {
                        if let Some(d) = command_payload.value.as_i64() {
                            info!("[Main Thread] Successfully received direction value: {}", d);
                            let (front_pulse, back_pulse) = direction_to_pulse(d);
                            info!(
                                "[Main Thread] Calculated pulses - Front: {}, Back: {}",
                                front_pulse, back_pulse
                            );

                            info!(
                                "[Main Thread] Setting front steering (channel {}) pulse to: {}",
                                FRONT_STEERING_CHANNEL, front_pulse
                            );
                            if let Err(e) =
                                controller.set_pwm(FRONT_STEERING_CHANNEL, 0, front_pulse)
                            {
                                error!("[Main Thread] Failed to set front steering PWM: {:?}", e);
                            }

                            info!(
                                "[Main Thread] Setting back steering (channel {}) pulse to: {}",
                                BACK_STEERING_CHANNEL, back_pulse
                            );
                            if let Err(e) = controller.set_pwm(BACK_STEERING_CHANNEL, 0, back_pulse)
                            {
                                error!("[Main Thread] Failed to set back steering PWM: {:?}", e);
                            }
                        } else {
                            warn!("[Main Thread] No direction value provided");
                        }
                    }
                    "headlights" => {
                        if let Some(h) = command_payload.value.as_bool() {
                            info!(
                                "[Main Thread] Successfully received headlights value: {}",
                                h
                            );
                            if let Some(pin) = lights.as_mut() {
                                if h {
                                    pin.set_high();
                                } else {
                                    pin.set_low();
                                }
                            } else {
                                warn!("[Main Thread] Headlights pin not available.");
                            }
                        } else {
                            warn!("[Main Thread] No headlights value provided");
                        }
                    }
                    "horn" => {
                        if let Some(h) = command_payload.value.as_bool() {
                            info!("[Main Thread] Successfully received horn value: {}", h);
                            if let Some(pin) = horn.as_mut() {
                                if h {
                                    pin.set_high();
                                } else {
                                    pin.set_low();
                                }
                            } else {
                                warn!("[Main Thread] Horn pin not available.");
                            }
                        } else {
                            warn!("[Main Thread] No horn value provided");
                        }
                    }
                    "turbo" => {
                        if let Some(t) = command_payload.value.as_bool() {
                            info!("[Main Thread] Successfully received turbo value: {}", t);
                            turbo = t;
                        } else {
                            warn!("[Main Thread] No turbo value provided");
                        }
                    }
                    "calibrate" => {
                        info!("[Main Thread] Calibrate command received. Setting steering to neutral.");
                        let (front_neutral, back_neutral) = direction_to_pulse(0);
                        controller
                            .set_pwm(FRONT_STEERING_CHANNEL, 0, front_neutral)
                            .unwrap_or_else(|e| {
                                error!("Failed to set front neutral: {:?}", e);
                            });
                        controller
                            .set_pwm(BACK_STEERING_CHANNEL, 0, back_neutral)
                            .unwrap_or_else(|e| {
                                error!("Failed to set back neutral: {:?}", e);
                            });
                        info!(
                            "[Main Thread] Steering set to neutral: Front {}, Back {}",
                            front_neutral, back_neutral
                        );
                    }
                    _ => {
                        warn!(
                            "[Main Thread] Unknown command received: {}",
                            command_payload.command
                        );
                    }
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

    info!("[Main Thread] Setting outputs to neutral/off before exit.");
    let (front_neutral, back_neutral) = direction_to_pulse(0);
    let _ = controller.set_pwm(FRONT_STEERING_CHANNEL, 0, front_neutral);
    let _ = controller.set_pwm(BACK_STEERING_CHANNEL, 0, back_neutral);
    let _ = controller.set_pwm(MOTOR_CHANNEL, 0, speed_to_pulse(0, false));
    if let Some(pin) = lights.as_mut() {
        pin.set_low();
    }
    if let Some(pin) = horn.as_mut() {
        pin.set_low();
    }

    info!("[Main Thread] Anwendung wird beendet.");
}

fn speed_to_pulse(speed: i64, turbo: bool) -> u16 {
    let x = speed.clamp(-100, 100);

    const NEUTRAL_PULSE: f32 = 1450.0;
    const DEAD_ZONE: i64 = 7;

    if (-DEAD_ZONE..=DEAD_ZONE).contains(&x) {
        NEUTRAL_PULSE as u16
    } else if x < -DEAD_ZONE {
        let slope = 200.0 / (100.0 - (DEAD_ZONE as f32 + 1.0));
        let pulse_val = NEUTRAL_PULSE + (x as f32 + DEAD_ZONE as f32) * slope;
        pulse_val.round() as u16
    } else {
        let slope = 750.0 / (100.0 - (DEAD_ZONE as f32 + 1.0));
        let pulse_val = NEUTRAL_PULSE + (x as f32 - DEAD_ZONE as f32) * slope;
        let mut final_pulse = pulse_val.round() as u16;
        if turbo {
            final_pulse = final_pulse.saturating_add(100);
        }
        final_pulse
    }
}

fn direction_to_pulse(direction: i64) -> (u16, u16) {
    let x = direction.clamp(-100, 100) as f32;

    let normalized_direction = (x + 100.0) / 200.0;

    let front_pulse = FRONT_LEFT_PULSE as f32 * (1.0 - normalized_direction)
        + FRONT_RIGHT_PULSE as f32 * normalized_direction;

    let back_pulse = BACK_LEFT_PULSE as f32 * (1.0 - normalized_direction)
        + BACK_RIGHT_PULSE as f32 * normalized_direction;

    (front_pulse.round() as u16, back_pulse.round() as u16)
}
