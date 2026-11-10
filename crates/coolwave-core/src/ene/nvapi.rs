//! SMBus transfers on an NVIDIA card's I2C ports through NVAPI.
//!
//! `nvapi64.dll` comes with the NVIDIA driver. It has one export,
//! `nvapi_QueryInterface`, which hands out every other function by a numeric
//! ID. The two I2C calls used here are not in NVIDIA's public headers; their
//! IDs and the request layout are the ones OpenRGB uses.
//!
//! A "bus" number here is `gpu index * PORTS_PER_GPU + port`.

use std::ffi::c_void;
use std::io;
use std::sync::OnceLock;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};

const ID_INITIALIZE: u32 = 0x0150_E828;
const ID_ENUM_PHYSICAL_GPUS: u32 = 0xE5AC_921F;
const ID_I2C_WRITE_EX: u32 = 0x283A_C65A;
const ID_I2C_READ_EX: u32 = 0x4D7B_0709;

const NVAPI_OK: i32 = 0;
const MAX_PHYSICAL_GPUS: usize = 64;
const PORTS_PER_GPU: u32 = 8;
/// Lighting controllers sit on port 1 on the cards seen so far, so it is tried first.
const PORT_ORDER: [u32; PORTS_PER_GPU as usize] = [1, 0, 2, 3, 4, 5, 6, 7];
/// Let the driver pick the bus speed.
const SPEED_DEFAULT_DEPRECATED: u32 = 0xFFFF;
const SPEED_DEFAULT_KHZ: u32 = 0;
/// Longest data part of one transfer: an SMBus block is a count byte and up to 32 bytes.
const MAX_DATA: usize = 33;

type GpuHandle = *mut c_void;
type QueryInterfaceFn = unsafe extern "C" fn(u32) -> *mut c_void;
type InitializeFn = unsafe extern "C" fn() -> i32;
type EnumGpusFn = unsafe extern "C" fn(*mut [GpuHandle; MAX_PHYSICAL_GPUS], *mut u32) -> i32;
type I2cFn = unsafe extern "C" fn(GpuHandle, *mut I2cRequest, *mut u32) -> i32;

/// NV_I2C_INFO_V3.
#[repr(C)]
struct I2cRequest {
    version: u32,
    display_mask: u32,
    is_ddc_port: u8,
    /// 7-bit address shifted left by one.
    device_address: u8,
    register: *mut u8,
    register_len: u32,
    data: *mut u8,
    data_len: u32,
    speed_deprecated: u32,
    speed_khz: u32,
    port: u8,
    port_is_set: u32,
}

const REQUEST_VERSION: u32 = std::mem::size_of::<I2cRequest>() as u32 | (3 << 16);

struct Nvapi {
    gpus: Vec<GpuHandle>,
    write: I2cFn,
    read: I2cFn,
}

// SAFETY: the handles are plain identifiers and NVAPI may be called from any thread.
unsafe impl Send for Nvapi {}
unsafe impl Sync for Nvapi {}

fn load() -> Option<Nvapi> {
    let name: Vec<u16> = "nvapi64.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: the library is only looked for in System32, where the driver installs it,
    // and each function pointer is cast to the signature NVAPI documents for that ID.
    unsafe {
        let library = LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if library.is_null() {
            return None;
        }
        let query: QueryInterfaceFn = std::mem::transmute(GetProcAddress(library, c"nvapi_QueryInterface".as_ptr().cast())?);
        let function = |id: u32| {
            let address = query(id);
            (!address.is_null()).then_some(address)
        };
        let initialize: InitializeFn = std::mem::transmute(function(ID_INITIALIZE)?);
        let enum_gpus: EnumGpusFn = std::mem::transmute(function(ID_ENUM_PHYSICAL_GPUS)?);
        let write: I2cFn = std::mem::transmute(function(ID_I2C_WRITE_EX)?);
        let read: I2cFn = std::mem::transmute(function(ID_I2C_READ_EX)?);
        if initialize() != NVAPI_OK {
            return None;
        }
        let mut handles = [std::ptr::null_mut(); MAX_PHYSICAL_GPUS];
        let mut count = 0u32;
        if enum_gpus(&mut handles, &mut count) != NVAPI_OK {
            return None;
        }
        Some(Nvapi { gpus: handles[..(count as usize).min(MAX_PHYSICAL_GPUS)].to_vec(), write, read })
    }
}

fn nvapi() -> io::Result<&'static Nvapi> {
    static NVAPI: OnceLock<Option<Nvapi>> = OnceLock::new();
    NVAPI.get_or_init(load).as_ref().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "NVIDIA driver (NVAPI) not available"))
}

/// Every port of every NVIDIA card, most likely port first.
pub fn gpu_buses() -> io::Result<Vec<u32>> {
    let gpus = nvapi()?.gpus.len() as u32;
    Ok((0..gpus).flat_map(|gpu| PORT_ORDER.iter().map(move |port| gpu * PORTS_PER_GPU + port)).collect())
}

pub fn no_access_hint(bus: u32) -> String {
    format!("no access to GPU I2C port {}", bus % PORTS_PER_GPU)
}

pub struct Bus {
    api: &'static Nvapi,
    gpu: GpuHandle,
    port: u8,
    address: u8,
}

// SAFETY: see `Nvapi`.
unsafe impl Send for Bus {}

enum Direction {
    Read,
    Write,
}

impl Bus {
    pub fn open(bus: u32, addr: u16) -> io::Result<Self> {
        let api = nvapi()?;
        let gpu = *api
            .gpus
            .get((bus / PORTS_PER_GPU) as usize)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such NVIDIA GPU"))?;
        Ok(Self { api, gpu, port: (bus % PORTS_PER_GPU) as u8, address: addr as u8 })
    }

    /// One I2C transfer: the optional register byte, then `data` written or read.
    fn transfer(&self, direction: Direction, register: Option<u8>, data: &mut [u8]) -> io::Result<()> {
        debug_assert!(data.len() <= MAX_DATA);
        let mut register_byte = register.unwrap_or(0);
        let mut request = I2cRequest {
            version: REQUEST_VERSION,
            display_mask: 0,
            is_ddc_port: 0,
            device_address: self.address << 1,
            register: &mut register_byte,
            register_len: register.is_some() as u32,
            data: data.as_mut_ptr(),
            data_len: data.len() as u32,
            speed_deprecated: SPEED_DEFAULT_DEPRECATED,
            speed_khz: SPEED_DEFAULT_KHZ,
            port: self.port,
            port_is_set: 1,
        };
        let call = match direction {
            Direction::Read => self.api.read,
            Direction::Write => self.api.write,
        };
        let mut unused = 0u32;
        // SAFETY: every pointer in `request` is valid for the length given with it.
        let status = unsafe { call(self.gpu, &mut request, &mut unused) };
        if status == NVAPI_OK { Ok(()) } else { Err(io::Error::other(format!("NVAPI I2C error {status}"))) }
    }

    pub fn read_byte(&self) -> io::Result<u8> {
        let mut data = [0u8];
        self.transfer(Direction::Read, None, &mut data)?;
        Ok(data[0])
    }

    pub fn read_byte_data(&self, cmd: u8) -> io::Result<u8> {
        let mut data = [0u8];
        self.transfer(Direction::Read, Some(cmd), &mut data)?;
        Ok(data[0])
    }

    pub fn write_byte_data(&self, cmd: u8, v: u8) -> io::Result<()> {
        self.transfer(Direction::Write, Some(cmd), &mut [v])
    }

    pub fn write_word_data(&self, cmd: u8, v: u16) -> io::Result<()> {
        self.transfer(Direction::Write, Some(cmd), &mut v.to_le_bytes())
    }

    /// SMBus block write: on the wire the data is preceded by its length.
    pub fn write_block_data(&self, cmd: u8, data: &[u8]) -> io::Result<()> {
        if data.len() >= MAX_DATA {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "SMBus block too long"));
        }
        let mut block = [0u8; MAX_DATA];
        block[0] = data.len() as u8;
        block[1..=data.len()].copy_from_slice(data);
        self.transfer(Direction::Write, Some(cmd), &mut block[..=data.len()])
    }
}
