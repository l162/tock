//! Component for kv storage Drivers.
//!
//! This provides one component, KVStoreComponent, which provides
//! a system call inteface to kv storage.
//!
//! Usage
//! -----
//! ```rust
//! let nonvolatile_storage = components::kv_store::KVStoreComponent::new(
//!     board_kernel,
//!     &sam4l::flashcalw::FLASH_CONTROLLER,
//!     0x60000,
//!     0x20000,
//!     &_sstorage as *const u8 as usize,
//!     &_estorage as *const u8 as usize,
//! )
//! .finalize(components::nv_storage_component_helper!(
//!     sam4l::flashcalw::FLASHCALW
//! ));
//! ```

use capsules::kv_store::KVStoreDriver;
use capsules::virtual_flash::FlashUser;
use capsules::virtual_flash::MuxFlash;
use core::mem::MaybeUninit;
use kernel::capabilities;
use kernel::component::Component;
use kernel::create_capability;
use kernel::hil;
use kernel::hil::flash::HasClient;
use kernel::static_init_half;

// Setup static space for the objects.
#[macro_export]
macro_rules! flash_user_component_helper {
    ($F:ty) => {{
        use capsules::virtual_flash::MuxFlash;
        use core::mem::MaybeUninit;
        static mut BUF1: MaybeUninit<MuxFlash<'static, $F>> = MaybeUninit::uninit();
        &mut BUF1
    };};
}

pub struct FlashMuxComponent<F: 'static + hil::flash::Flash> {
    flash: &'static F,
}

impl<F: 'static + hil::flash::Flash> FlashMuxComponent<F> {
    pub fn new(flash: &'static F) -> FlashMuxComponent<F> {
        FlashMuxComponent { flash }
    }
}

impl<F: 'static + hil::flash::Flash> Component for FlashMuxComponent<F> {
    type StaticInput = &'static mut MaybeUninit<MuxFlash<'static, F>>;
    type Output = &'static MuxFlash<'static, F>;

    unsafe fn finalize(self, s: Self::StaticInput) -> Self::Output {
        let mux_flash = static_init_half!(s, MuxFlash<'static, F>, MuxFlash::new(self.flash));

        mux_flash
    }
}

// Setup static space for the objects.
#[macro_export]
macro_rules! kv_store_component_helper {
    ($F:ty, $S:ty) => {{
        use capsules::kv_store::KVStoreDriver;
        use capsules::virtual_flash::FlashUser;
        use core::mem::MaybeUninit;
        use kernel::hil;
        static mut BUF1: MaybeUninit<FlashUser<'static, $F>> = MaybeUninit::uninit();
        static mut BUF2: MaybeUninit<KVStoreDriver<'static, FlashUser<'static, $F>, $S>> =
            MaybeUninit::uninit();
        (&mut BUF1, &mut BUF2)
    };};
}

pub struct KVStoreComponent<F: 'static + hil::flash::Flash, const S: usize> {
    board_kernel: &'static kernel::Kernel,
    mux_flash: &'static MuxFlash<'static, F>,
    region_offset: usize,
    length: usize,
    read_buf: &'static mut [u8; S],
    page_buffer: &'static mut F::Page,
}

impl<F: 'static + hil::flash::Flash, const S: usize> KVStoreComponent<F, { S }> {
    pub fn new(
        board_kernel: &'static kernel::Kernel,
        mux_flash: &'static MuxFlash<'static, F>,
        region_offset: usize,
        length: usize,
        read_buf: &'static mut [u8; S],
        page_buffer: &'static mut F::Page,
    ) -> Self {
        Self {
            board_kernel,
            mux_flash,
            region_offset,
            length,
            read_buf,
            page_buffer,
        }
    }
}

impl<F: 'static + hil::flash::Flash, const S: usize> Component for KVStoreComponent<F, { S }> {
    type StaticInput = (
        &'static mut MaybeUninit<FlashUser<'static, F>>,
        &'static mut MaybeUninit<KVStoreDriver<'static, FlashUser<'static, F>, S>>,
    );
    type Output = &'static KVStoreDriver<'static, FlashUser<'static, F>, S>;

    unsafe fn finalize(self, static_buffer: Self::StaticInput) -> Self::Output {
        let grant_cap = create_capability!(capabilities::MemoryAllocationCapability);

        let virtual_flash = static_init_half!(
            static_buffer.0,
            FlashUser<'static, F>,
            FlashUser::new(self.mux_flash)
        );

        let driver = static_init_half!(
            static_buffer.1,
            KVStoreDriver<'static, FlashUser<'static, F>, S>,
            KVStoreDriver::new(
                virtual_flash,
                self.board_kernel.create_grant(&grant_cap),
                self.read_buf,
                self.length,
                self.page_buffer,
                self.region_offset,
            )
        );
        virtual_flash.set_client(driver);
        driver.initalise();
        driver
    }
}
