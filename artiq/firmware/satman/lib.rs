#![feature(compiler_builtins_lib)]
#![no_std]

extern crate compiler_builtins;
extern crate alloc_artiq;
#[macro_use]
extern crate std_artiq as std;
#[macro_use]
extern crate log;
extern crate logger_artiq;
extern crate board;
extern crate drtioaux;

// FIXME: is there a better way of doing this?
trait FromU16 {
    fn from_u16(v: u16) -> Self;
}

impl FromU16 for u16 {
    fn from_u16(v: u16) -> u16 {
        v
    }
}

impl FromU16 for u8 {
    fn from_u16(v: u16) -> u8 {
        v as u8
    }
}

fn u8_or_u16<T: FromU16>(v: u16) -> T {
    FromU16::from_u16(v)
}

fn process_aux_packet(p: &drtioaux::Packet) {
    match *p {
        drtioaux::Packet::EchoRequest => drtioaux::hw::send(&drtioaux::Packet::EchoReply).unwrap(),
        drtioaux::Packet::MonitorRequest { channel, probe } => {
            let value;
            #[cfg(has_rtio_moninj)]
            unsafe {
                board::csr::rtio_moninj::mon_chan_sel_write(u8_or_u16(channel));
                board::csr::rtio_moninj::mon_probe_sel_write(probe);
                board::csr::rtio_moninj::mon_value_update_write(1);
                value = board::csr::rtio_moninj::mon_value_read();
            }
            #[cfg(not(has_rtio_moninj))]
            {
                value = 0;
            }
            let reply = drtioaux::Packet::MonitorReply { value: value as u32 };
            drtioaux::hw::send(&reply).unwrap();
        },
        drtioaux::Packet::InjectionRequest { channel, overrd, value } => {
            #[cfg(has_rtio_moninj)]
            unsafe {
                board::csr::rtio_moninj::inj_chan_sel_write(u8_or_u16(channel));
                board::csr::rtio_moninj::inj_override_sel_write(overrd);
                board::csr::rtio_moninj::inj_value_write(value);
            }
        },
        drtioaux::Packet::InjectionStatusRequest { channel, overrd } => {
            let value;
            #[cfg(has_rtio_moninj)]
            unsafe {
                board::csr::rtio_moninj::inj_chan_sel_write(u8_or_u16(channel));
                board::csr::rtio_moninj::inj_override_sel_write(overrd);
                value = board::csr::rtio_moninj::inj_value_read();
            }
            #[cfg(not(has_rtio_moninj))]
            {
                value = 0;
            }
            let reply = drtioaux::Packet::InjectionStatusReply { value: value };
            drtioaux::hw::send(&reply).unwrap();
        },
        _ => warn!("received unexpected aux packet {:?}", p)
    }
}

fn process_aux_packets() {
    let pr = drtioaux::hw::recv();
    match pr {
        Ok(None) => {},
        Ok(Some(p)) => process_aux_packet(&p),
        Err(e) => warn!("aux packet error ({})", e)
    }
}


#[cfg(rtio_frequency = "62.5")]
const SI5324_SETTINGS: board::si5324::FrequencySettings
        = board::si5324::FrequencySettings {
    n1_hs  : 10,
    nc1_ls : 8,
    n2_hs  : 10,
    n2_ls  : 20112,
    n31    : 2514,
    n32    : 4597,
    bwsel  : 4
};

#[cfg(rtio_frequency = "150.0")]
const SI5324_SETTINGS: board::si5324::FrequencySettings
        = board::si5324::FrequencySettings {
    n1_hs  : 9,
    nc1_ls : 4,
    n2_hs  : 10,
    n2_ls  : 33732,
    n31    : 9370,
    n32    : 7139,
    bwsel  : 3
};

fn drtio_link_is_up() -> bool {
    unsafe {
        board::csr::drtio::link_status_read() == 1
    }
}

fn startup() {
    board::clock::init();
    info!("ARTIQ satellite manager starting...");
    info!("software version {}", include_str!(concat!(env!("OUT_DIR"), "/git-describe")));
    info!("gateware version {}", board::ident(&mut [0; 64]));

    #[cfg(has_ad9516)]
    board::ad9516::init().expect("cannot initialize ad9516");
    board::i2c::init();
    board::si5324::setup(&SI5324_SETTINGS).expect("cannot initialize si5324");

    loop {
        while !drtio_link_is_up() {}
        info!("link is up, switching to recovered clock");
        board::si5324::select_ext_input(true).expect("failed to switch clocks");
        while drtio_link_is_up() {
            process_aux_packets();
        }
        info!("link is down, switching to local crystal clock");
        board::si5324::select_ext_input(false).expect("failed to switch clocks");
    }
}

#[no_mangle]
pub extern fn main() -> i32 {
    unsafe {
        extern {
            static mut _fheap: u8;
            static mut _eheap: u8;
        }
        alloc_artiq::seed(&mut _fheap as *mut u8,
                          &_eheap as *const u8 as usize - &_fheap as *const u8 as usize);

        static mut LOG_BUFFER: [u8; 65536] = [0; 65536];
        logger_artiq::BufferLogger::new(&mut LOG_BUFFER[..]).register(startup);
        0
    }
}

#[no_mangle]
pub extern fn exception_handler(vect: u32, _regs: *const u32, pc: u32, ea: u32) {
    panic!("exception {:?} at PC 0x{:x}, EA 0x{:x}", vect, pc, ea)
}

#[no_mangle]
pub extern fn abort() {
    panic!("aborted")
}

// Allow linking with crates that are built as -Cpanic=unwind even if we use -Cpanic=abort.
// This is never called.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn _Unwind_Resume() -> ! {
    loop {}
}
