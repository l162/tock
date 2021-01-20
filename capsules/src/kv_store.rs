//! TODO

use crate::driver;
/// Syscall driver number.
pub const DRIVER_NUM: usize = driver::NUM::KVStore as usize;

use core::cell::Cell;
use core::hash::SipHasher;
use kernel::common::cells::{OptionalCell, TakeCell};
use kernel::debug;
use kernel::hil::flash::{self, Client, Flash};
use kernel::{AppId, AppSlice, Callback, Driver, Grant, ReturnCode, Shared};
use tickv;
use tickv::success_codes::SuccessCode;
use tickv::AsyncTicKV;
use tickv::TicKV;

#[derive(Clone, Copy)]
pub enum State {
    None,
    ReadComplete(isize),
    WriteComplete(usize),
    EraseComplete(usize),
}

#[derive(Clone, Copy)]
enum Operation {
    None,
    Init,
    GetKey,
}

pub struct TicKVFlashCtrl<'a, F: Flash + 'static, const S: usize> {
    pub flash: &'a F,
    pub data_buffer: TakeCell<'static, F::Page>,
    pub state: Cell<State>,
    pub region_offset: usize,
}

impl<'a, F: Flash, const S: usize> TicKVFlashCtrl<'a, F, { S }> {
    pub fn new(
        flash: &'a F,
        data_buffer: &'static mut F::Page,
        region_offset: usize,
    ) -> TicKVFlashCtrl<'a, F, { S }> {
        Self {
            flash,
            data_buffer: TakeCell::new(data_buffer),
            state: Cell::new(State::None),
            region_offset,
        }
    }
}

impl<'a, F: Flash, const S: usize> tickv::flash_controller::FlashController<{ S }>
    for TicKVFlashCtrl<'a, F, { S }>
{
    fn read_region(
        &self,
        region_number: usize,
        _offset: usize,
        buf: &mut [u8; S],
    ) -> Result<(), tickv::error_codes::ErrorCode> {
        match self.state.get() {
            State::ReadComplete(reg) => {
                if reg as usize == region_number {
                    // We already have read the data.
                    let data_buf = self.data_buffer.take().unwrap();
                    for (i, d) in data_buf.as_mut().iter().enumerate() {
                        buf[i] = *d;
                    }
                    self.data_buffer.replace(data_buf);
                    self.state.set(State::None);

                    return Ok(());
                }

                if self
                    .flash
                    .read_page(
                        self.region_offset + region_number,
                        self.data_buffer.take().unwrap(),
                    )
                    .is_err()
                {
                    return Err(tickv::error_codes::ErrorCode::ReadFail);
                }
            }
            _ => {
                if self
                    .flash
                    .read_page(
                        self.region_offset + region_number,
                        self.data_buffer.take().unwrap(),
                    )
                    .is_err()
                {
                    return Err(tickv::error_codes::ErrorCode::ReadFail);
                }
            }
        }

        Err(tickv::error_codes::ErrorCode::ReadNotReady(region_number))
    }

    fn write(&self, address: usize, buf: &[u8]) -> Result<(), tickv::error_codes::ErrorCode> {
        let data_buf = self.data_buffer.take().unwrap();

        for (i, d) in buf.iter().enumerate() {
            data_buf.as_mut()[i] = *d;
        }

        if self
            .flash
            .write_page((0x20040000 + address) / 1024, data_buf)
            .is_err()
        {
            return Err(tickv::error_codes::ErrorCode::WriteFail);
        }

        Err(tickv::error_codes::ErrorCode::WriteNotReady(address))
    }

    fn erase_region(&self, region_number: usize) -> Result<(), tickv::error_codes::ErrorCode> {
        self.flash.erase_page(self.region_offset + region_number);

        Err(tickv::error_codes::ErrorCode::EraseNotReady(region_number))
    }
}

pub struct KVStoreDriver<'a, F: Flash + 'static, const S: usize> {
    pub inner: AsyncTicKV<'a, TicKVFlashCtrl<'a, F, S>, SipHasher, S>,
    apps: Grant<App>,
    appid: OptionalCell<AppId>,
    operation: Cell<Operation>,
}

impl<'a, F: Flash, const S: usize> KVStoreDriver<'a, F, { S }> {
    pub fn new(
        flash: &'a F,
        grant: Grant<App>,
        read_buf: &'static mut [u8; S],
        region_size: usize,
        data_buffer: &'static mut F::Page,
        region_offset: usize,
    ) -> KVStoreDriver<'a, F, { S }> {
        let tickv = AsyncTicKV::<TicKVFlashCtrl<F, S>, SipHasher, S>::new(
            TicKVFlashCtrl::new(flash, data_buffer, region_offset),
            read_buf,
            region_offset
        );

        Self {
            inner: tickv,
            apps: grant,
            appid: OptionalCell::empty(),
            operation: Cell::new(Operation::None),
        }
    }

    pub fn initalise(&self) {
        let ret = self
            .inner
            .initalise((&mut SipHasher::new(), &mut SipHasher::new()));

        let state = match ret {
            Err(tickv::error_codes::ErrorCode::ReadNotReady(reg)) => {
                // Read isn't ready, save the state
                State::ReadComplete(reg as isize)
            }
            Err(tickv::error_codes::ErrorCode::EraseNotReady(reg)) => {
                // Erase isn't ready, save the state
                State::EraseComplete(reg)
            }
            _ => State::None,
            //Ok(()) => State::None,
            //_ => unreachable!(),
        };

        self.inner.tickv.controller.state.set(state);
        self.operation.set(Operation::Init);
    }

    //fn update_state(&self, ret: Result<(), tickv::error_codes::ErrorCode>) {
    fn update_state(&self, ret: Result<SuccessCode, tickv::error_codes::ErrorCode>) {
        let state = match ret {
            Err(tickv::error_codes::ErrorCode::ReadNotReady(reg)) => {
                // Read isn't ready, save the state
                State::ReadComplete(reg as isize)
            }
            Err(tickv::error_codes::ErrorCode::EraseNotReady(reg)) => {
                // Erase isn't ready, save the state
                State::EraseComplete(reg)
            }
            Err(tickv::error_codes::ErrorCode::WriteNotReady(reg)) => {
                // Write isn't ready, save the state
                State::WriteComplete(reg)
            }
            Ok(_) => {
                self.operation.set(Operation::None);
                State::None
            }
            Err(e) => panic!("Error: {:?}", e),
        };

        self.inner.tickv.controller.state.set(state);
    }
}

impl<'a, F: Flash, const S: usize> Client<F> for KVStoreDriver<'a, F, { S }> {
    fn read_complete(&self, pagebuffer: &'static mut F::Page, _error: flash::Error) {
        self.inner.tickv.controller.data_buffer.replace(pagebuffer);

        match self.operation.get() {
            Operation::Init => {
                let ret = self
                    .inner
                    .continue_operation((&mut SipHasher::new(), &mut SipHasher::new()))
                    .0;

                self.update_state(ret);

                if ret.is_ok() {
                    self.operation.set(Operation::None);
                }
            }
            Operation::GetKey => {
                let data_buf = self.inner.tickv.controller.data_buffer.take().unwrap();
                self.inner.tickv.controller.data_buffer.replace(data_buf);

                self.appid.map(|id| {
                    self.apps
                        .enter(*id, |app, _| {
                            if let Some(key) = app.key.take() {
                                if let Some(mut value) = app.value.take() {
                                    let key_len = app.key_len.take().unwrap();

                                    let ret = self
                                        .inner
                                        .continue_operation((
                                            &mut SipHasher::new(),
                                            &mut SipHasher::new(),
                                        ))
                                        .0;

                                    self.update_state(ret);

                                    if ret.is_ok() {
                                        self.appid.map(|id| {
                                            self.apps
                                                .enter(*id, |app, _| {
                                                    app.callback.map(|cb| {
                                                        cb.schedule(0, 0, 0);
                                                    });
                                                })
                                                .unwrap();
                                        });

                                        self.operation.set(Operation::None);
                                    }
                                    app.key_len.set(key_len);
                                    app.value.replace(value);
                                    app.key.replace(key);
                                }
                            }
                        })
                        .unwrap();
                });
            }
            _ => unreachable!(),
        }
    }

    fn write_complete(&self, pagebuffer: &'static mut F::Page, _error: flash::Error) {
        self.inner.tickv.controller.data_buffer.replace(pagebuffer);
        self.inner.tickv.controller.state.set(State::None);

        match self.operation.get() {
            Operation::Init => {}
            _ => unreachable!(),
        }

        self.operation.set(Operation::None);
    }

    fn erase_complete(&self, _error: flash::Error) {
        match self.operation.get() {
            Operation::Init => {
                let ret = self
                    .inner
                    .continue_operation((&mut SipHasher::new(), &mut SipHasher::new()))
                    .0;

                self.update_state(ret);
            }
            _ => unreachable!(),
        }
    }
}

impl<'a, F: Flash, const S: usize> Driver for KVStoreDriver<'a, F, { S }> {
    /// Specify memory regions to be used.
    ///
    /// ### `allow_num`
    ///
    fn allow(
        &self,
        appid: AppId,
        allow_num: usize,
        slice: Option<AppSlice<Shared, u8>>,
    ) -> ReturnCode {
        match allow_num {
            // Key buffer
            0 => self
                .apps
                .enter(appid, |app, _| {
                    app.key = slice;
                    ReturnCode::SUCCESS
                })
                .unwrap_or(ReturnCode::FAIL),

            // Value buffer
            1 => self
                .apps
                .enter(appid, |app, _| {
                    app.value = slice;
                    ReturnCode::SUCCESS
                })
                .unwrap_or(ReturnCode::FAIL),

            // default
            _ => ReturnCode::ENOSUPPORT,
        }
    }

    /// Subscribe to events.
    ///
    /// ### `subscribe_num`
    ///
    fn subscribe(
        &self,
        subscribe_num: usize,
        callback: Option<Callback>,
        appid: AppId,
    ) -> ReturnCode {
        match subscribe_num {
            0 => {
                // set callback
                self.apps
                    .enter(appid, |app, _| {
                        app.callback.insert(callback);
                        ReturnCode::SUCCESS
                    })
                    .unwrap_or(ReturnCode::FAIL)
            }

            // default
            _ => ReturnCode::ENOSUPPORT,
        }
    }

    /// Access the KV Store
    ///
    /// ### `command_num`
    ///
    fn command(
        &self,
        command_num: usize,
        key_len: usize,
        _data2: usize,
        appid: AppId,
    ) -> ReturnCode {
        match command_num {
            // Append key
            0 => ReturnCode::SUCCESS,

            // Get key
            1 => self
                .apps
                .enter(appid, |app, _| {
                    if let Some(key) = app.key.take() {
                        if let Some(mut value) = app.value.take() {
                            self.appid.set(appid);
                            self.operation.set(Operation::GetKey);
                            app.key_len.set(key_len);

                            let ret = self.inner.tickv.get_key(
                                &mut SipHasher::new(),
                                &key.as_ref()[0..key_len],
                                value.as_mut(),
                            );

                            self.update_state(ret);

                            app.value.replace(value);
                            app.key.replace(key);

                            ReturnCode::SUCCESS
                        } else {
                            app.key.replace(key);
                            ReturnCode::EBUSY
                        }
                    } else {
                        ReturnCode::EBUSY
                    }
                })
                .unwrap_or_else(|err| err.into()),

            // Invalidate ke
            2 => ReturnCode::SUCCESS,

            // Trigger garbage collection
            3 => ReturnCode::SUCCESS,

            // default
            _ => ReturnCode::ENOSUPPORT,
        }
    }
}

pub struct App {
    callback: OptionalCell<Callback>,
    _pending_run_app: Option<AppId>,
    key: Option<AppSlice<Shared, u8>>,
    key_len: OptionalCell<usize>,
    value: Option<AppSlice<Shared, u8>>,
}

impl Default for App {
    fn default() -> App {
        App {
            callback: OptionalCell::empty(),
            _pending_run_app: None,
            key: None,
            key_len: OptionalCell::empty(),
            value: None,
        }
    }
}
