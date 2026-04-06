use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, Device, EventSummary, EventType, InputEvent, InputId, KeyCode, PropType, RelativeAxisCode, UinputAbsSetup, uinput::VirtualDevice
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

    // set up virtual touchpad for absolute mouse movements
    let touch_axis_info = AbsInfo::new(0, -100, 100, 0, 0, 200);
    let mut touchpad = create_touchpad(touch_axis_info, &(VJOYSTICK_NAME.to_owned()+"_pad")).unwrap();

    // fetch events and send them through to virtual joystick
    let min: i32 = conf.range_min();
    let max: i32 = conf.range_max();
    let mut mouse_x_pos: i32 = 0;
    let mut joystick_x_pos: i32 = 0;
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
            mouse2joy_active = true;
            let _ = mouse.grab();
            let _ = mouse.send_events(&[InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, 0)]);
            let _ = mouse.send_events(&[InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, 0)]);
            let _ = mouse.ungrab();
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

fn create_touchpad(abs_info: AbsInfo, name: &str) -> std::io::Result<VirtualDevice> {
    let max_x = 1920;
    let max_y = 1080;

    let abs_setup_x = AbsInfo::new(0, 0, max_x, 0, 0, 0);
    let abs_setup_y = AbsInfo::new(0, 0, max_y, 0, 0, 0);
    
    let abs_pressure = AbsInfo::new(0, 0, 100, 0, 0, 0); //dummy

    let mut buttons = AttributeSet::<KeyCode>::new();
    buttons.insert(KeyCode::BTN_TOUCH);
    buttons.insert(KeyCode::BTN_TOOL_FINGER); //dummy
    buttons.insert(KeyCode::BTN_LEFT); //dummy
    buttons.insert(KeyCode::BTN_RIGHT); //dummy
    buttons.insert(KeyCode::BTN_MIDDLE); //dummy

    let mut prop = AttributeSet::<PropType>::new();
    prop.insert(PropType::POINTER);

    let fakeid = InputId::new(BusType::BUS_USB, 0x2, 0x8, 0x200);

    let touchpad = VirtualDevice::builder()?
        .name(name)
        .input_id(fakeid)
        .with_keys(&buttons)?
        .with_properties(&prop)?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, abs_setup_x))?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, abs_setup_y))?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_PRESSURE, abs_pressure))?
        .build()?;

    Ok(touchpad)
}

fn touchpad_touch(x:i32, y:i32, pad:&mut VirtualDevice) {
    let m0_x = create_abs(AbsoluteAxisCode::ABS_X, 0);
    let m0_y = create_abs(AbsoluteAxisCode::ABS_Y, 0);
    

    let pressure_down = create_abs(AbsoluteAxisCode::ABS_PRESSURE, 80);

    let start_touch = InputEvent::new(1, KeyCode::BTN_TOUCH.0, 1);
    let start_finger = InputEvent::new(1, KeyCode::BTN_TOOL_FINGER.0, 1);

    const MAX_EV: i32 = 30;
    const INI: i32 = 5;
    let step_x = x/MAX_EV;
    let step_y = y/MAX_EV;
    

    let mut count:i32 = INI;
    let mut event_arr: [InputEvent; (MAX_EV+INI) as usize] = [start_touch; (MAX_EV+INI) as usize];
    event_arr[1] = start_finger;
    event_arr[2] = pressure_down;
    event_arr[3] = m0_x;
    event_arr[4] = m0_y;
    while count < MAX_EV + INI {
        event_arr[(count+1) as usize] = pressure_down;
        event_arr[(count+1) as usize] = create_abs(AbsoluteAxisCode::ABS_X, count*step_x);
        event_arr[(count+2) as usize] = create_abs(AbsoluteAxisCode::ABS_Y, count*step_y);
        count += 3;
    }

    match pad.emit(&event_arr) {
        Ok(_) => {info!("touchpad!")},
        Err(e) => {warn!(":( Error: {}", e)}
    }


    let pressure_up = create_abs(AbsoluteAxisCode::ABS_PRESSURE, 0);
    let stop_touch = InputEvent::new(1, KeyCode::BTN_TOUCH.0, 0);
    let stop_finger = InputEvent::new(1, KeyCode::BTN_TOOL_FINGER.0, 0);
    pad.emit(&[stop_touch, pressure_up, stop_finger]).unwrap()
}

fn create_abs(code:AbsoluteAxisCode, val:i32) -> InputEvent {
    let event = InputEvent::new(EventType::ABSOLUTE.0, code.0, val);
    return event;
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
