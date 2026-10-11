// Minimal VA-API video post-processing (VPP) helper: scale + colour convert a
// source surface into a destination surface on the GPU. Added for OpenUUYC's
// Linux host capture path (BGRX/BGRA full-res -> NV12 encode size).
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

    /// Scale + colour-convert `src` into `dst`. NULL regions mean "whole
    /// surface" (required: some iHD builds ignore shallow-copied rect pointers
    /// from vaCreateBuffer and leave the destination untouched / gray).
    /// Blocks until the destination surface is ready for encode.
    pub fn blit(
        &self,
        src: bindings::VASurfaceID,
        _src_wh: (u32, u32),
        dst: bindings::VASurfaceID,
        _dst_wh: (u32, u32),
    ) -> Result<(), VaError> {
        // Zero-init every field; dangling filter/surface pointers from Default
        // on some bindgen layouts have been observed to make iHD skip the blit.
        let mut param: bindings::VAProcPipelineParameterBuffer = unsafe { std::mem::zeroed() };
        param.surface = src;
        // NULL = whole surface. When OPENUUYC_VPP_OUTREGION=1, pin heap rects so
        // the pointers stay valid through vaRenderPicture and crop to the exact
        // target size instead of scaling into the dst alignment padding.
        let mut _src_rect; let mut _dst_rect;
        if std::env::var_os("OPENUUYC_VPP_OUTREGION").is_some() {
            _src_rect = Box::new(bindings::VARectangle { x: 0, y: 0, width: _src_wh.0 as u16, height: _src_wh.1 as u16 });
            _dst_rect = Box::new(bindings::VARectangle { x: 0, y: 0, width: _dst_wh.0 as u16, height: _dst_wh.1 as u16 });
            param.surface_region = &*_src_rect as *const _;
            param.output_region = &*_dst_rect as *const _;
        } else {
            param.surface_region = std::ptr::null();
            param.output_region = std::ptr::null();
        }
        // RGB (sRGB full-range) source -> BT.601 limited NV12 output by default.
        // Env overrides let us tune without rebuilding:
        //   OPENUUYC_VPP_SRCCS  (int VAProcColorStandardType, default 8=SRGB)
        //   OPENUUYC_VPP_DSTCS  (default 1=BT601)
        //   OPENUUYC_VPP_SRCRANGE (1=reduced,2=full; default 2=full for RGB)
        //   OPENUUYC_VPP_DSTRANGE (default 1=reduced for TV-range NV12)
        let env_u32 = |k: &str, d: u32| -> u32 {
            std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
        };
        param.surface_color_standard = env_u32(
            "OPENUUYC_VPP_SRCCS",
            bindings::_VAProcColorStandardType_VAProcColorStandardSRGB,
        );
        param.output_color_standard = env_u32(
            "OPENUUYC_VPP_DSTCS",
            bindings::_VAProcColorStandardType_VAProcColorStandardBT601,
        );
        param.input_color_properties.color_range =
            env_u32("OPENUUYC_VPP_SRCRANGE", bindings::VA_SOURCE_RANGE_FULL) as u8;
        param.output_color_properties.color_range =
            env_u32("OPENUUYC_VPP_DSTRANGE", bindings::VA_SOURCE_RANGE_REDUCED) as u8;
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

        // Source must be idle before VPP reads it (after CPU PutImage/derive).
        va_check(unsafe { bindings::vaSyncSurface(self.display.handle(), src) })?;

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
