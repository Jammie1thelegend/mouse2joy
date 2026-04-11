use evdev::{
    AbsInfo, AbsoluteAxisCode, Device, EventSummary, EventType, InputEvent, KeyCode, RelativeAxisCode, UinputAbsSetup, uinput::VirtualDevice
};
use std::fs;
use thiserror::Error;
use log::{info, warn, error, LevelFilter};
use env_logger::Builder;

mod configuration;
use configuration::Config;

const VJOYSTICK_NAME: &str = "mouse2joy";

// virtual joystick buttons, won't be used but increase chances of joystick being recognized
static KEYS: [KeyCode; 14] = [
    KeyCode::BTN_EAST,
    KeyCode::BTN_SOUTH,
    KeyCode::BTN_NORTH,
    KeyCode::BTN_WEST,
    KeyCode::BTN_DPAD_UP,
    KeyCode::BTN_DPAD_DOWN,
    KeyCode::BTN_DPAD_LEFT,
    KeyCode::BTN_DPAD_RIGHT,
    KeyCode::BTN_SELECT,
    KeyCode::BTN_START,
    KeyCode::BTN_TL,
    KeyCode::BTN_TR,
    KeyCode::BTN_TL2,
    KeyCode::BTN_TR2,
];

#[derive(Error, Debug)]
pub enum Mouse2JoyError {
    #[error("Failed to find a mouse device. Make sure you are running the application with root priviledges.")]
    NoMouseError,

    #[error("Failed to read a mouse input")]
    FailedToReadInput,
}

fn main() -> Result<(), Mouse2JoyError> {

    // initialize logger
    Builder::new()
        .filter_level(LevelFilter::Trace)  // This shows everything
        .init();

    let conf = load_config();
    info!("sensitivity: {}", conf.sensitivity);
    
    // find all input devices that can be used as a mouse
    let mut mouse_devices: Vec<Device> = fs::read_dir("/dev/input")
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.path().into_os_string().to_str().map(String::from))
        .filter_map(|path| {
            Device::open(&path)
                .ok()
                .filter(|device| device.supported_events().contains(EventType::RELATIVE))
        })
        .collect();
    
    if mouse_devices.is_empty() {
        error!("{}", Mouse2JoyError::NoMouseError);
        return Err(Mouse2JoyError::NoMouseError);
    }

    // ask user which mouse to use
    if !(mouse_devices.len() == 1) {
        println!("Several mice detected, please select one:");
        for (i, mouse) in mouse_devices.iter().enumerate() {
            println!("{}: {}", i + 1, mouse.name().unwrap_or("Unknown Device"));
        }
    }

    let index = input_in_range(1, mouse_devices.len());
    let mut mouse = mouse_devices.remove(index - 1);
    info!("Using \"{}\" as input device", mouse.name().unwrap_or("Unknown Device"));

    // set up virtual joystick
    let axis_info = AbsInfo::new(conf.value(), conf.range_min(), conf.range_max(), conf.fuzz(), conf.flat(), conf.resolution());
    let mut joystick = create_joystick(axis_info, VJOYSTICK_NAME).unwrap();
    info!("Virtual joystick created");

    // fetch events and send them through to virtual joystick
    let min: i32 = conf.range_min();
    let max: i32 = conf.range_max();
    let mut mouse_x_pos: i32 = 0;
    let mut joystick_x_pos: i32;
    let mut shortcut: [bool; 2] = [false, false]; // right, middle click
    let mut mouse2joy_active: bool = false;
    loop {
        for ev in mouse.fetch_events().unwrap(){
                match ev.destructure() {
                    EventSummary::RelativeAxis(_, RelativeAxisCode::REL_X, _) => {
                        if mouse2joy_active == false {continue}
                        mouse_x_pos += ev.value();
                        joystick_x_pos = mouse_x_pos;

                        //touchpad_touch(joystick_x_pos, 1080/2, &mut touchpad);

                        if joystick_x_pos < min {
                            joystick_x_pos = min
                        }
                        if joystick_x_pos > max {
                            joystick_x_pos = max
                        }
                        let ev = InputEvent::new(
                            EventType::ABSOLUTE.0,
                            AbsoluteAxisCode::ABS_X.0,
                            joystick_x_pos,
                        );
                        match joystick.emit(&[ev]) {
                          Ok(_) => {
                            info!("Moved joystick position to {}", joystick_x_pos);
                          },
                          Err(e) => {
                            warn!("Failed to emit joystick event: {}", e);
                            continue;
                          }
                        }
                    },

                    EventSummary::Key(_, KeyCode::BTN_RIGHT, 1) => { shortcut[0] = true},
                    EventSummary::Key(_, KeyCode::BTN_RIGHT, 0) => { shortcut[0] = false},

                    EventSummary::Key(_, KeyCode::BTN_LEFT, 1) => { shortcut[1] = true},
                    EventSummary::Key(_, KeyCode::BTN_LEFT, 0) => { shortcut[1] = false},

                    _ => {}
                }
            };
        
        if shortcut == [true, true] {
            if mouse2joy_active == true {
                mouse2joy_active = false;
                warn!("mouse2joy off");
                warn!("!! Program still running, press ctrl+C to stop.");
                continue
            }
            warn!("mouse2joy on");
            let _ = mouse.grab();
            let _ = mouse.send_events(&mouse_move_evs(0, 0));
            let _ = mouse.ungrab();
            mouse2joy_active = true;
            }
         }
    }

fn create_joystick(abs_info: AbsInfo, name: &str) -> std::io::Result<VirtualDevice> {
    let abs_x = UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, abs_info);
    let abs_y = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, abs_info);

    let mut keys = evdev::AttributeSet::new();
    for button in KEYS {
        keys.insert(button)
    }

    let joystick = VirtualDevice::builder()?
        .name(name)
        .with_absolute_axis(&abs_x)?
        .with_absolute_axis(&abs_y)?
        .with_keys(&keys)?
        .build()?;

    Ok(joystick)
}

fn mouse_move_evs(x:i32, y:i32) -> [InputEvent; 2] {
    let event_x = InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, x);
    let event_y = InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, y);
    let events: [InputEvent; 2] = [event_x, event_y];
    return events;
}

// ask user for a usize input within a given range
fn input_in_range(min: usize, max: usize) -> usize {
    let mut input = String::new();

    loop {
        input.clear();
        std::io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");

        match input.trim().parse::<usize>() {
            Ok(index) if index >= min && index <= max => {
                return index;
            }
            _ => {
                println!(
                    "Invalid selection. Please enter a number between {} and {}",
                    min, max
                );
                continue;
            }
        }
    }
}

fn load_config() -> Config {
    if Config::exists() {
      match Config::load() {
        Ok(conf) => {
          info!("Using configuration file {}", Config::path());
          conf
        }
        Err(_) => {
          warn!("Problem loading the configuration, using default");
          Config::default()
        }
      }
    } else {
      info!("No configuration found, using default");
      Config::default()
    }
}
