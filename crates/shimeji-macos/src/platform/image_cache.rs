//! Per-character `CGImage`s from decoded RGBA. Shared by every instance.

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;

use super::hit_test::AlphaMask;

use objc2::runtime::AnyObject;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
};
use shimeji_core::model::Character;
use shimeji_packs::DecodedImage;

/// One character's frames. Mirroring is a layer transform unless the pack
/// shipped right-facing art (looked up in [`super::poses::PoseTable`]).
pub struct ImageCache {
    images: HashMap<String, CFRetained<CGImage>>,
    masks: HashMap<String, Arc<AlphaMask>>,
    aliases: HashMap<String, String>,
    decoded_bytes: u64,
}

impl ImageCache {
    pub fn from_import(character: &Character, images: &[DecodedImage]) -> Self {
        let mut cache = Self {
            images: HashMap::new(),
            masks: HashMap::new(),
            aliases: HashMap::new(),
            decoded_bytes: 0,
        };
        let root = character.image_root.trim_matches('/');
        for image in images {
            cache.decoded_bytes += image.rgba.len() as u64;
            if let Some(cg) = rgba_to_cg(image) {
                cache.masks.insert(
                    normalize(&image.path),
                    Arc::new(AlphaMask::from_rgba(&image.rgba, image.width, image.height)),
                );
                cache.insert(root, &image.path, cg);
            }
        }
        cache
    }

    pub fn decoded_bytes(&self) -> u64 {
        self.decoded_bytes
    }

    fn insert(&mut self, root: &str, path: &str, image: CFRetained<CGImage>) {
        let canon = normalize(path);
        self.images.insert(canon.clone(), image);
        for alias in aliases(root, path) {
            self.aliases.insert(alias, canon.clone());
        }
    }

    pub fn get(&self, key: &str) -> Option<&CGImage> {
        let canon = self
            .aliases
            .get(key)
            .or_else(|| self.aliases.get(&normalize(key)))
            .cloned()
            .unwrap_or_else(|| normalize(key));
        self.images
            .get(&canon)
            .or_else(|| self.images.get(key))
            .map(|image| &**image)
    }

    pub fn mask(&self, key: &str) -> Option<Arc<AlphaMask>> {
        let canon = self
            .aliases
            .get(key)
            .or_else(|| self.aliases.get(&normalize(key)))
            .cloned()
            .unwrap_or_else(|| normalize(key));
        self.masks.get(&canon).cloned()
    }

    pub fn as_id(&self, key: &str) -> Option<&AnyObject> {
        let image = self.get(key)?;
        Some(unsafe { &*std::ptr::from_ref(image).cast::<AnyObject>() })
    }
}

fn aliases(root: &str, path: &str) -> Vec<String> {
    let mut keys = vec![path.to_string(), normalize(path), format!("/{path}")];
    if let Some(name) = Path::new(path).file_name() {
        let name = name.to_string_lossy();
        keys.push(name.to_string());
        keys.push(format!("/{name}"));
        if !root.is_empty() {
            keys.push(format!("{root}/{name}"));
            keys.push(format!("/{root}/{name}"));
        }
    }
    keys
}

fn normalize(path: &str) -> String {
    path.trim().trim_start_matches('/').replace('\\', "/")
}

fn rgba_to_cg(image: &DecodedImage) -> Option<CFRetained<CGImage>> {
    let mut bytes = image.rgba.clone();
    premultiply(&mut bytes);
    let width = image.width as usize;
    let height = image.height as usize;
    let boxed = Box::new(bytes);
    let ptr = boxed.as_ptr();
    let len = boxed.len();
    let info = Box::into_raw(boxed);
    let provider =
        unsafe { CGDataProvider::with_data(info.cast(), ptr.cast(), len, Some(release_bytes)) }?;
    let space = CGColorSpace::new_device_rgb()?;
    unsafe {
        CGImage::new(
            width,
            height,
            8,
            32,
            width * 4,
            Some(&space),
            CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0),
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
}

fn premultiply(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        pixel[0] = ((u16::from(pixel[0]) * alpha) / 255) as u8;
        pixel[1] = ((u16::from(pixel[1]) * alpha) / 255) as u8;
        pixel[2] = ((u16::from(pixel[2]) * alpha) / 255) as u8;
    }
}

unsafe extern "C-unwind" fn release_bytes(info: *mut c_void, _data: NonNull<c_void>, _size: usize) {
    if !info.is_null() {
        drop(unsafe { Box::from_raw(info.cast::<Vec<u8>>()) });
    }
}
