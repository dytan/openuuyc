// Minimal VA-API video post-processing (VPP) helper: scale + colour convert a
// source surface into a destination surface on the GPU. Added for OpenUUYC's
// Linux host capture path (BGRX full-res -> NV12 encode size).
use std::ffi::c_void;
use std::rc::Rc;

use crate::bindings;
use crate::va_check;
use crate::{Display, VaError};

/// A reusable VPP (VAEntrypointVideoProc) context.
pub struct VppContext {
    display: Rc<Display>,
    config_id: bindings::VAConfigID,
    context_id: bindings::VAContextID,
}

impl VppContext {
    /// Create a VPP context sized for the destination surface.
    pub fn new(display: Rc<Display>, width: u32, height: u32) -> Result<Self, VaError> {
        let mut config_id: bindings::VAConfigID = 0;
        va_check(unsafe {
            bindings::vaCreateConfig(
                display.handle(),
                bindings::VAProfile::VAProfileNone,
                bindings::VAEntrypoint::VAEntrypointVideoProc,
                std::ptr::null_mut(),
                0,
                &mut config_id,
            )
        })?;
        let mut context_id: bindings::VAContextID = 0;
        let r = va_check(unsafe {
            bindings::vaCreateContext(
                display.handle(),
                config_id,
                width as i32,
                height as i32,
                0,
                std::ptr::null_mut(),
                0,
                &mut context_id,
            )
        });
        if let Err(e) = r {
            unsafe { bindings::vaDestroyConfig(display.handle(), config_id) };
            return Err(e);
        }
        Ok(Self {
            display,
            config_id,
            context_id,
        })
    }

    /// Scale + colour-convert `src` into `dst`. Regions cover the whole of each
    /// surface at the given sizes; the driver picks the conversion matrix from
    /// the surface fourccs, with BT.601 limited-range luma to match the software
    /// path. Blocks until the destination surface is ready.
    pub fn blit(
        &self,
        src: bindings::VASurfaceID,
        src_wh: (u32, u32),
        dst: bindings::VASurfaceID,
        dst_wh: (u32, u32),
    ) -> Result<(), VaError> {
        let src_rect = bindings::VARectangle {
            x: 0,
            y: 0,
            width: src_wh.0 as u16,
            height: src_wh.1 as u16,
        };
        let dst_rect = bindings::VARectangle {
            x: 0,
            y: 0,
            width: dst_wh.0 as u16,
            height: dst_wh.1 as u16,
        };
        let mut param: bindings::VAProcPipelineParameterBuffer = Default::default();
        param.surface = src;
        param.surface_region = &src_rect;
        param.output_region = &dst_rect;
        // 1 = BT.601 (see VAProcColorStandardType); matches the CPU CSC path.
        param.surface_color_standard = 1;
        param.output_color_standard = 1;
        param.filter_flags = bindings::VA_FILTER_SCALING_HQ;

        let mut buf_id: bindings::VABufferID = 0;
        va_check(unsafe {
            bindings::vaCreateBuffer(
                self.display.handle(),
                self.context_id,
                bindings::VABufferType::VAProcPipelineParameterBufferType,
                std::mem::size_of::<bindings::VAProcPipelineParameterBuffer>() as u32,
                1,
                &mut param as *mut _ as *mut c_void,
                &mut buf_id,
            )
        })?;

        let begin = va_check(unsafe {
            bindings::vaBeginPicture(self.display.handle(), self.context_id, dst)
        });
        let render = if begin.is_ok() {
            va_check(unsafe {
                bindings::vaRenderPicture(self.display.handle(), self.context_id, &mut buf_id, 1)
            })
        } else {
            Ok(())
        };
        let end = if begin.is_ok() {
            va_check(unsafe { bindings::vaEndPicture(self.display.handle(), self.context_id) })
        } else {
            Ok(())
        };
        unsafe { bindings::vaDestroyBuffer(self.display.handle(), buf_id) };
        begin?;
        render?;
        end?;
        va_check(unsafe { bindings::vaSyncSurface(self.display.handle(), dst) })?;
        Ok(())
    }
}

impl Drop for VppContext {
    fn drop(&mut self) {
        unsafe {
            bindings::vaDestroyContext(self.display.handle(), self.context_id);
            bindings::vaDestroyConfig(self.display.handle(), self.config_id);
        }
    }
}
