use bitvec::prelude::*;
use std::{io, mem, num::NonZeroU8, str, sync::Arc};

use crate::{Button, Hid, HP_SIGNATURE};

fn u16_from_bytes(low: u8, high: u8) -> u16 {
    u16::from_le_bytes([low, high])
}

#[derive(Default, Clone, Copy)]
struct Header {
    signature: u16,
    composit_device: u8,
    length: usize,
    sequence: u8,
}

impl Header {
    fn new(data: &[u8]) -> Option<Self> {
        Some(Self {
            signature: u16_from_bytes(*data.get(0)?, *data.get(1)? & 0b1111),
            composit_device: (data.get(1)? >> 4) & 0b1111,
            length: u16_from_bytes(*data.get(2)?, *data.get(3)? & 0b11) as usize,
            sequence: (*data.get(3)? >> 2) & 0b111111,
        })
    }

    fn kind(&self) -> Option<u16> {
        self.signature.checked_sub(HP_SIGNATURE)
    }
}

#[derive(Debug)]
pub enum Event {
    Firmware {
        version: (u16, u16, u16),
        device: String,
        serial: String,
    },
    Battery {
        low_level: u8,
        crit_level: u8,
        power_off_timeout: u8,
        auto_report_delay: u8,
        level: u8,
    },
    Buttons {
        total_buttons: u8,
        programmed_buttons: u8,
        host_id: u8,
        support_long_press: bool,
        support_double_press: bool,
        support_down_up_press: bool,
        support_simulate: bool,
        support_program_stop: bool,
        buttons: Vec<Button>,
    },
    Mouse {
        max_dpi: u16,
        min_dpi: u16,
        dpi: u16,
        step_dpi: u16,
        nb_sensitivity_wheel1: Option<NonZeroU8>,
        sensitivity_wheel1: u8,
        nb_sensitivity_wheel2: Option<NonZeroU8>,
        sensitivity_wheel2: u8,
        host_id: u8,
        cut_off_max: u8,
        cut_off: u8,
        support_left_handed: bool,
        left_handed: bool,
        support_no_save_to_flash: bool,
    },
}

pub struct HpMouseEventIterator {
    dev: Arc<Hid>,
    incoming: Vec<u8>,
    header: Header,
}

impl HpMouseEventIterator {
    pub(crate) fn new(dev: Arc<Hid>) -> Self {
        Self {
            dev,
            incoming: Vec::new(),
            header: Header::default(),
        }
    }

    fn report_1_packet_1(&mut self, data: &[u8]) -> Option<Event> {
        println!("Update {}", data.len());

        if data.len() <= 3 {
            // Buffer too small
            return None;
        }

        let firmware_version = u16_from_bytes(data[0], data[1]);
        let major_version = firmware_version / 1000;
        let minor_version = (firmware_version % 1000) / 10;
        let patch_version = firmware_version % 10;

        let mut items = Vec::with_capacity(2);
        let mut i = 4;
        while i < data.len() {
            let size = data[i] as usize;
            i += 1;

            let mut item = Vec::with_capacity(size);
            while i < data.len() && item.len() < size {
                item.push(data[i]);
                i += 1;
            }
            items.push(item);
        }

        let device = str::from_utf8(items.get(0)?).ok()?;
        let serial = str::from_utf8(items.get(1)?).ok()?;

        Some(Event::Firmware {
            version: (major_version, minor_version, patch_version),
            device: device.to_string(),
            serial: serial.to_string(),
        })
    }

    fn report_1_packet_6(&mut self, data: &[u8]) -> Option<Event> {
        if data.len() <= 4 {
            // Buffer too small
            return None;
        }

        let low_level = data[0];
        let crit_level = data[1];
        let power_off_timeout = data[2];
        let auto_report_delay = data[3];
        let level = data[4];

        Some(Event::Battery {
            low_level,
            crit_level,
            power_off_timeout,
            auto_report_delay,
            level,
        })
    }

    fn report_1_packet_14(&mut self, data: &[u8]) -> Option<Event> {
        if data.get(0) != Some(&0) {
            // Wrong command
            return None;
        }

        if data.len() <= 4 {
            // Buffer too small
            return None;
        }

        let total_buttons = data[1];
        let programmed_buttons = data[2];
        let host_id = data[3];

        let flags = data[4].view_bits::<Lsb0>();
        let support_long_press = flags[0];
        let support_double_press = flags[1];
        let support_down_up_press = flags[2];
        let support_simulate = flags[3];
        let support_program_stop = flags[4];

        let mut buttons = Vec::with_capacity(programmed_buttons as usize);
        let mut i = 5;
        while buttons.len() < programmed_buttons as usize {
            if data.len() <= i + 3 {
                // Buffer too small
                break;
            }

            let size = data[i + 3] as usize;
            let mut button = Button {
                id: data[i + 0],
                host_id: data[i + 1],
                press_type: data[i + 2],
                action: Vec::with_capacity(size),
            };
            i += 4;

            while i < data.len() && button.action.len() < size {
                button.action.push(data[i]);
                i += 1;
            }
            buttons.push(button);
        }

        for button in buttons.iter() {
            eprintln!("Action: {:?}", button.decode_action());
        }

        Some(Event::Buttons {
            total_buttons,
            programmed_buttons,
            host_id,
            support_long_press,
            support_double_press,
            support_down_up_press,
            support_simulate,
            support_program_stop,
            buttons,
        })
    }

    fn report_1_packet_18(&mut self, data: &[u8]) -> Option<Event> {
        if data.get(0) != Some(&0) {
            // Wrong command
            return None;
        }

        if data.len() <= 14 {
            // Buffer too small
            return None;
        }

        let max_dpi = u16_from_bytes(data[1], data[2]);
        let min_dpi = u16_from_bytes(data[3], data[4]);
        let dpi = u16_from_bytes(data[5], data[6]);
        let step_dpi = u16_from_bytes(data[7], data[8]);

        let nb_sensitivity_wheel1 = NonZeroU8::new(data[9] & 0b1111);
        let sensitivity_wheel1 = data[9] >> 4;
        let nb_sensitivity_wheel2 = NonZeroU8::new(data[10] & 0b1111);
        let sensitivity_wheel2 = data[10] >> 4;

        let host_id = data[11];
        let cut_off_max = data[12];
        let cut_off = data[13];

        let flags = data[14].view_bits::<Lsb0>();
        let support_left_handed = flags[0];
        let left_handed = flags[1];
        let support_no_save_to_flash = flags[2];

        Some(Event::Mouse {
            max_dpi,
            min_dpi,
            dpi,
            step_dpi,
            nb_sensitivity_wheel1,
            sensitivity_wheel1,
            nb_sensitivity_wheel2,
            sensitivity_wheel2,
            host_id,
            cut_off_max,
            cut_off,
            support_left_handed,
            left_handed,
            support_no_save_to_flash,
        })
    }

    fn report_1(&mut self, data: &[u8]) -> Option<Event> {
        let header = Header::new(data)?;

        let kind_opt = header.kind();
        println!(
            " signature {:04X} {:?} length {} sequence {}",
            header.signature, kind_opt, header.length, header.sequence
        );

        // Ensure signature is valid and can be converted to a packet kind
        let kind = kind_opt?;

        //TODO: replace asserts with errors

        // Insert new incoming packet if sequence is 0, assert that there is no current one
        if header.sequence == 0 {
            assert_eq!(self.incoming.len(), 0);
            self.header = header;
        // Get current incoming packet, assert that it exists
        } else {
            assert_eq!(header.signature, self.header.signature);
            assert_eq!(header.length, self.header.length);
            assert_eq!(header.sequence, self.header.sequence + 1);
            self.header.sequence += 1;
        }

        // Push back new data
        self.incoming.extend_from_slice(&data[4..]);

        // If we received enough data, truncate and return
        if self.incoming.len() >= header.length {
            let mut incoming = mem::take(&mut self.incoming);
            incoming.truncate(header.length);
            return match kind {
                1 => self.report_1_packet_1(&incoming),
                6 => self.report_1_packet_6(&incoming),
                14 => self.report_1_packet_14(&incoming),
                18 => self.report_1_packet_18(&incoming),
                _ => None,
            };
        }

        // No full packet yet
        None
    }
}

impl Iterator for HpMouseEventIterator {
    type Item = io::Result<Event>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = [0; 4096];
        loop {
            let len = match self.dev.read(&mut buf) {
                Ok(len) => len,
                Err(err) => {
                    return Some(Err(err));
                }
            };
            eprintln!("HID read {}", len);

            if len == 0 {
                return None;
            }

            for i in 0..len {
                eprint!(" {:02x}", buf[i]);
            }
            eprintln!();

            match buf[0] {
                1 => match self.report_1(&buf[1..len]) {
                    Some(event) => {
                        return Some(Ok(event));
                    }
                    None => {}
                },
                _ => {}
            }
        }
    }
}
