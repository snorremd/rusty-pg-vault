use anyhow::Result;
use aws_sdk_s3::Client;
use aws_sdk_s3::types::{Object, CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::config::{Builder, Credentials, Region, BehaviorVersion};
use aws_sdk_s3::primitives::ByteStream;
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt};
use bytes::Bytes;
use std::boxed::Box;

const PART_SIZE: usize = 1024 * 1024; // 1MB - minimum allowed by S3
const READ_BUFFER_SIZE: usize = 8 * 1024; // 8KB

#[async_trait]
pub trait S3ClientTrait {
    async fn list_objects(&self, prefix: Option<String>) -> Result<Vec<Object>>;
    async fn upload_stream<R>(&self, key: &str, reader: R) -> Result<()>
    where
        R: AsyncRead + Send + Unpin + 'static;
}

#[derive(Clone)]
pub struct S3Client {
    client: Client,
    bucket: String,
}

#[async_trait]
impl S3ClientTrait for S3Client {
    async fn list_objects(&self, prefix: Option<String>) -> Result<Vec<Object>> {
        let mut all_objects = Vec::new();
        let mut continuation_token = None;

        loop {
            let mut request = self.client
                .list_objects_v2()
                .bucket(&self.bucket)
                .set_prefix(prefix.clone());

            if let Some(token) = &continuation_token {
                request = request.continuation_token(token);
            }

            let response = request.send().await?;
            
            if let Some(contents) = response.contents {
                all_objects.extend(contents);
            }

            // Check if there are more results
            continuation_token = response.next_continuation_token;
            if continuation_token.is_none() {
                break;
            }
        }

        Ok(all_objects)
    }

    async fn upload_stream<R>(&self, key: &str, reader: R) -> Result<()>
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let mut reader = reader;

        // Create multipart upload
        let create_resp = self.client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        let upload_id = create_resp.upload_id().unwrap();

        // Process chunks and upload parts
        let mut buffer = Vec::with_capacity(PART_SIZE);
        let mut parts = Vec::new();
        let mut part_number = 1;
        let mut chunk = [0u8; READ_BUFFER_SIZE];

        loop {
            let n = reader.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..n]);


            // Upload part as soon as we have enough data
            if buffer.len() >= PART_SIZE {
                let part_data = buffer.split_off(PART_SIZE);
                let part_resp = self.client
                    .upload_part()
                    .bucket(&self.bucket)
                    .key(key)
                    .upload_id(&*upload_id)
                    .part_number(part_number)
                    .body(buffer.into())
                    .send()
                    .await?;

                parts.push(
                    CompletedPart::builder()
                        .set_e_tag(part_resp.e_tag().map(|s| s.to_string()))
                        .part_number(part_number)
                        .build(),
                );

                buffer = part_data;
                part_number += 1;
            }
        }

        // Upload final part if there's remaining data
        if !buffer.is_empty() {
            let part_resp = self.client
                .upload_part()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(&*upload_id)
                .part_number(part_number)
                .body(buffer.into())
                .send()
                .await?;

            parts.push(
                CompletedPart::builder()
                    .set_e_tag(part_resp.e_tag().map(|s| s.to_string()))
                    .part_number(part_number)
                    .build(),
            );
        }

        // Complete multipart upload
        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(&*upload_id)
            .multipart_upload(CompletedMultipartUpload::builder().set_parts(Some(parts)).build())
            .send()
            .await?;

        Ok(())
    }
}

impl S3Client {
    pub fn new(
        region: String,
        access_key_id: String,
        secret_access_key: String,
        endpoint_url: String,
        bucket: String,
    ) -> Self {
        let region = Region::new(region);
        
        let credentials = Credentials::new(
            &access_key_id,
            &secret_access_key,
            None,
            None,
            "rusty-pg-vault",
        );

        let mut config_builder = Builder::new()
            .region(region)
            .credentials_provider(credentials)
            .behavior_version(BehaviorVersion::latest());

        // Add endpoint if provided
        if !endpoint_url.is_empty() {
            config_builder = config_builder.endpoint_url(endpoint_url);
        }

        let config = config_builder.build();
        let client = Client::from_conf(config);

        Self { client, bucket }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;
    use mockall::mock;

    mock! {
        S3ClientTrait {
            fn list_objects(&self, prefix: Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Object>>> + Send>>;
            fn upload_stream<R>(&self, key: &str, reader: R) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
            where
                R: AsyncRead + Send + Unpin + 'static;
        }
    }

    #[tokio::test]
    async fn test_list_objects() {
        let mut mock_client = MockS3ClientTrait::new();
        mock_client.expect_list_objects()
            .with(eq(Some("test-prefix".to_string())))
            .returning(|_| Box::pin(async { Ok(vec![]) }));

        let result = mock_client.list_objects(Some("test-prefix".to_string())).await;
        assert!(result.is_ok());
    }
}
