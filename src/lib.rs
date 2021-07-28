use std::ffi::CStr;
use skyline::{hook, install_hook};

pub static mut DEVICE_INITIALIZE_OFFS: *const u8 = 0 as _;
pub static mut QUEUE_SUBMIT_COMMANDS_OFFS: *const u8 = 0 as _;
pub static mut DEVICE: *const nvn::Device = 0 as _;


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
pub extern "C" fn device_initialize(device: *const nvn::Device, device_builder: *const nvn::DeviceBuilder) -> bool {
    println!("Custom nvnDeviceInitialize called");

    unsafe { DEVICE = device };

    let result = original!()(device, device_builder);
    
    assert_eq!(result, true, "nvnDeviceInitialize returned false");
    unsafe { QUEUE_SUBMIT_COMMANDS_OFFS = nvnBootstrapLoader(skyline::c_str("nvnQueueSubmitCommands\0")) };
    install_hook!(queue_submit_commands);

    result
}

#[hook(replace = QUEUE_SUBMIT_COMMANDS_OFFS)]
pub extern "C" fn queue_submit_commands(queue: &mut nvn::Queue, count: usize, handles: *mut nvn::CommandHandle) {
    println!("Custom nvnQueueSubmitCommands called");

    let device = unsafe { &*DEVICE };

    let mut command_handles = unsafe { std::slice::from_raw_parts_mut(handles, count) };
    let mut test: Vec<nvn::CommandHandle> = vec![];
    test.extend_from_slice(command_handles);

    // Create a MemoryPoolBuilder for our MemoryPool
    let mut mem_builder = nvn::MemoryPoolBuilder::new();

    mem_builder.set_defaults();
    mem_builder.set_device(device);
    mem_builder.set_flags(nvn::MemoryPoolFlags::new().with_cpu_uncached(true).with_gpu_cached(true));
    mem_builder.set_storage(unsafe { libc::memalign(0x1000, 0x1000) } as _, 0x1000);

    let mut mem_pool = nvn::MemoryPool::new();
    mem_pool.initialize(&mem_builder);

    // Create our CommandBuffer
    let mut comm_buf = nvn::CommandBuffer::new();
    // TODO: Set Command and Control memory here. 0x1000 in size for both.
    comm_buf.add_command_memory(&mem_pool, 0, 0x1000);
    comm_buf.add_control_memory(unsafe { libc::memalign(0x1000, 0x1000) } as _, 0x1000);
    // Initialize the CommandBuffer now that everything is ready
    assert_eq!(comm_buf.initialize(device), true, "Couldn't initialize CommandBuffer");
    // Start recording commands to scissor and clear an area of the screen
    println!("Begin Recording for CommandBuffer");
    comm_buf.begin_recording();
    comm_buf.set_scissor(0, 0, 512, 512);
    //comm_buf.clear_color(0, 0 as _, o as _);
    // Stop recording, get your CommandHandle
    let handle = comm_buf.end_recording();
    println!("Ended recording, handle acquired: {:#?}", handle);

    // TODO: Append our new CommandHandler to the list
    println!("Command handles count: {}", count);
    
    original!()(queue, count, handles)
}

#[skyline::main(name = "nvn_research")]
pub fn main() {
    install_hook!(bootstrap_loader);
}