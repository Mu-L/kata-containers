// Copyright (c) 2023 Intel Corporation
//
// SPDX-License-Identifier: Apache-2.0
//

use super::new_device;
use crate::confidential_data_hub;
use crate::confidential_data_hub::image::{is_sandbox, unpack_pause_image};
use crate::rpc::CONTAINER_BASE;
use crate::storage::{StorageContext, StorageHandler};
use anyhow::{anyhow, Result};
use kata_types::mount::KATA_VIRTUAL_VOLUME_IMAGE_GUEST_PULL;
use kata_types::mount::{ImagePullVolume, StorageDevice};
use protocols::agent::Storage;
use safe_path::scoped_join;
use std::sync::Arc;
use tracing::instrument;

#[derive(Debug)]
pub struct ImagePullHandler {}

impl ImagePullHandler {
    fn get_image_info(storage: &Storage) -> Result<ImagePullVolume> {
        for option in storage.driver_options.iter() {
            if let Some((key, value)) = option.split_once('=') {
                if key == KATA_VIRTUAL_VOLUME_IMAGE_GUEST_PULL {
                    let imagepull_volume: ImagePullVolume = serde_json::from_str(value)?;
                    return Ok(imagepull_volume);
                }
            }
        }
        Err(anyhow!("missing Image information for ImagePull volume"))
    }
}

#[async_trait::async_trait]
impl StorageHandler for ImagePullHandler {
    #[instrument]
    fn driver_types(&self) -> &[&str] {
        &[KATA_VIRTUAL_VOLUME_IMAGE_GUEST_PULL]
    }

    #[instrument]
    async fn create_device(
        &self,
        storage: Storage,
        ctx: &mut StorageContext,
    ) -> Result<Arc<dyn StorageDevice>> {
        //Currently the image metadata is not used to pulling image in the guest.
        let image_pull_volume = Self::get_image_info(&storage)?;
        debug!(ctx.logger, "image_pull_volume = {:?}", image_pull_volume);
        let image_name = storage.source();
        debug!(ctx.logger, "image_name = {:?}", image_name);

        let cid = ctx
            .cid
            .clone()
            .ok_or_else(|| anyhow!("failed to get container id"))?;

        info!(
            ctx.logger,
            "image metadata: {:?}", image_pull_volume.metadata
        );
        if is_sandbox(&image_pull_volume.metadata) {
            let mount_path = unpack_pause_image(&cid)?;
            return new_device(mount_path);
        }

        // generated bundles with rootfs and config.json will store under CONTAINER_BASE/cid/images.
        let bundle_path = scoped_join(CONTAINER_BASE, &cid)?;
        let bundle_path = match confidential_data_hub::pull_image(image_name, bundle_path).await {
            Ok(path) => {
                info!(
                    ctx.logger,
                    "pull and unpack image {image_name}, cid: {cid} succeeded."
                );
                path
            }
            Err(e) => {
                error!(
                    ctx.logger,
                    "pull and unpack image {image_name}, cid: {cid} failed with {:?}.",
                    e.to_string()
                );
                return Err(e);
            }
        };

        new_device(bundle_path)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use kata_types::mount::{ImagePullVolume, KATA_VIRTUAL_VOLUME_IMAGE_GUEST_PULL};
    use protocols::agent::Storage;

    use crate::storage::image_pull_handler::ImagePullHandler;

    #[test]
    fn test_get_image_info() {
        let mut res = HashMap::new();
        res.insert("key1".to_string(), "value1".to_string());
        res.insert("key2".to_string(), "value2".to_string());

        let image_pull = ImagePullVolume {
            metadata: res.clone(),
        };

        let image_pull_str = serde_json::to_string(&image_pull);
        assert!(image_pull_str.is_ok());

        let storage = Storage {
            driver: KATA_VIRTUAL_VOLUME_IMAGE_GUEST_PULL.to_string(),
            driver_options: vec![format!("image_guest_pull={}", image_pull_str.ok().unwrap())],
            ..Default::default()
        };

        match ImagePullHandler::get_image_info(&storage) {
            Ok(image_info) => {
                assert_eq!(image_info.metadata, res);
            }
            Err(e) => panic!("err = {}", e),
        }
    }
}
