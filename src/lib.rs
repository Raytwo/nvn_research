use core::panic;
use std::ffi::CStr;
use skyline::{hook, install_hook, install_hooks};
use lazy_static::lazy_static;
use parking_lot::Mutex;

const COMMAND_BUFFER_COUNT: usize = 3;

pub static mut DEVICE_INITIALIZE_OFFS: *const u8 = 0 as _;
pub static mut QUEUE_SUBMIT_COMMANDS_OFFS: *const u8 = 0 as _;
pub static mut SET_TEXTURE_OFFS: *const u8 = 0 as _;
pub static mut ACQUIRE_TEXTURE_OFFS: *const u8 = 0 as _;
pub static mut TEXTURES: Vec<*mut nvn::Texture> = Vec::new();
pub static mut TEXTURE_IDX: i32 = 0;


lazy_static! {
    static ref GLOBAL_MEMPOOL: Mutex<nvn::managed::MemPool> = {
        let builder = nvn::managed::MemPool::new()
            .with_device(nvn::global_device())
            .with_flags(nvn::MemoryPoolFlags::new().with_cpu_uncached(true).with_gpu_cached(true))
            .make_storage(0x1000 * COMMAND_BUFFER_COUNT, None);
        Mutex::new(builder.finish().unwrap())
    };

    static ref COMMAND_BUFFERS: Mutex<Vec<nvn::managed::CommandBuffer>> = {
        let mut vec = Vec::new();
        let mut mempool = GLOBAL_MEMPOOL.lock();
        for _ in 0..COMMAND_BUFFER_COUNT {
            let cmd = mempool.reserve_mem(0x1000).unwrap().into_range();
            vec.push(
                nvn::managed::CommandBuffer::new()
                    .with_device(nvn::global_device())
                    .make_control(0x1000, None)
                    .with_command(mempool.as_mut(), cmd.start, cmd.end - cmd.start)
                    .finish()
                    .unwrap()
            );
        }
        Mutex::new(vec)
    };
}

extern "C" {
    #[link_name = "\u{1}nvnBootstrapLoader"]
    pub fn nvnBootstrapLoader(func: *const u8) -> *const u8;
}

#[hook(replace = nvnBootstrapLoader)]
pub fn bootstrap_loader(func: *const u8) -> *const u8 {
    let function_name = unsafe { CStr::from_ptr(func as _).to_str().unwrap() };

    println!("nvnBootstrapLoader request: {}", function_name);

    match function_name {
        "nvnDeviceInitialize" => unsafe {
            if DEVICE_INITIALIZE_OFFS.is_null() {
                DEVICE_INITIALIZE_OFFS = original!()(func);

                // We need to hook it because returning our own function pointer does not work
                install_hook!(device_initialize);
            }

            DEVICE_INITIALIZE_OFFS as _
        },
        _ => original!()(func),
    }
}

#[hook(replace = DEVICE_INITIALIZE_OFFS)]
pub fn device_initialize(device: *mut nvn::Device, device_builder: *const nvn::DeviceBuilder) -> bool {
    nvn::set_global_device(device);

    let result = original!()(device, device_builder);
    
    assert_eq!(result, true, "nvnDeviceInitialize returned false");

    unsafe { QUEUE_SUBMIT_COMMANDS_OFFS = nvnBootstrapLoader(skyline::c_str("nvnQueueSubmitCommands\0")) };
    unsafe { SET_TEXTURE_OFFS = nvnBootstrapLoader(skyline::c_str("nvnWindowBuilderSetTextures\0")) };
    unsafe { ACQUIRE_TEXTURE_OFFS = nvnBootstrapLoader(skyline::c_str("nvnWindowAcquireTexture\0")) };


    install_hooks!(queue_submit_commands, set_textures, acquire_texture);

    result
}

#[hook(replace = SET_TEXTURE_OFFS)]
pub fn set_textures(builder: *const u8, count: i32, textures: *const *mut nvn::Texture) {
    if textures != 0 as _ {
        let texs = unsafe { std::slice::from_raw_parts(textures, count as _) }.to_vec();
        unsafe { TEXTURES = texs };
        //dbg!(&texs);
        //println!("cock: {}", texs.len());
    }
    
    original!()(builder, count, textures);
}

#[hook(replace = ACQUIRE_TEXTURE_OFFS)]
pub fn acquire_texture(window: *const u8, texture_available: *const u8, texture_idx: *const i32) -> i32 {
    unsafe { TEXTURE_IDX = *texture_idx };
    original!()(window, texture_available, texture_idx)
}

#[hook(replace = QUEUE_SUBMIT_COMMANDS_OFFS)]
pub fn queue_submit_commands(queue: &nvn::Queue, count: usize, handles: *mut nvn::CommandHandle) {
    if unsafe { TEXTURES.len() != 0} {
        let texture = unsafe { TEXTURES[TEXTURE_IDX as usize] };
        let render_target: [*const nvn::Texture;1] = [texture];

        // Clone all the handles in a vector for ease of use
        let command_handles = unsafe { std::slice::from_raw_parts(handles, count) };
        let mut new_handles: Vec<nvn::CommandHandle> = vec![];
        new_handles.extend_from_slice(command_handles);

        if let Some(command_buf) = COMMAND_BUFFERS.lock().get_mut(unsafe { TEXTURE_IDX as usize }) {
            command_buf.reset();
            let clear_color: &[f32;4] = &[1.0, 1.0, 0.0, 1.0];
    
            // Start recording commands to scissor and clear an area of the screen
            println!("Begin Recording for CommandBuffer");
            command_buf.begin_recording();
    
            command_buf.set_render_targets(1, &render_target as _, 0 as _, 0 as _, 0 as _);
            command_buf.set_scissor(0, 0, 420, 420);
            command_buf.set_viewport(0, 0, 420, 420);
            // TODO: Fix ClearColorMask
            command_buf.clear_color(0, clear_color as _, nvn::ClearColorMask::new().with_r(true).with_g(true).with_b(true));
    
            // Stop recording, store our CommandHandle at the beginning of the vec
            new_handles.insert(0, command_buf.end_recording());
    
            // Call the original function with our new handle appended
            let new_count = new_handles.len();
            original!()(queue, new_count, new_handles.as_ptr() as _);
        }

    }
    else {
        original!()(queue, count, handles);
    }
}

#[skyline::main(name = "nvn_research")]
pub fn main() {
    install_hook!(bootstrap_loader);
}