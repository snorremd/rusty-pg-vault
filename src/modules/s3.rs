use anyhow::Result;
use aws_sdk_s3::Client;
use aws_sdk_s3::types::{Object, CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::config::{Builder, Credentials, Region, BehaviorVersion};
use aws_sdk_s3::primitives::ByteStream;
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use std::boxed::Box;
use crate::cli::S3Config;

const PART_SIZE: usize = 1024 * 1024 * 5; // 5MB - minimum allowed by certain S3 providers

#[async_trait]
pub trait S3ClientTrait {
    async fn list_objects(&self, prefix: Option<String>) -> Result<Vec<Object>>;
    async fn upload_to_s3_streaming<R>(&self, reader: R, key: &str) -> Result<()>
    where
        R: AsyncRead + Unpin + Send + 'static;
    async fn download_from_s3_streaming(&self, key: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>>;
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

    async fn upload_to_s3_streaming<R>(&self, reader: R, key: &str) -> Result<()>
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let create_resp = self.client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;

        let upload_id = create_resp.upload_id().unwrap();
        let mut completed_parts: Vec<CompletedPart> = Vec::new();
        let mut part_number = 1;

        let mut reader = BufReader::with_capacity(PART_SIZE, reader);
        let mut buffer = Vec::with_capacity(PART_SIZE);
        let mut temp = vec![0u8; 8192]; // smaller internal read buffer

        loop {
            let n = reader.read(&mut temp).await?;
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&temp[..n]);

            while buffer.len() >= PART_SIZE {
                let part_data = buffer.drain(..PART_SIZE).collect::<Vec<u8>>();

                let part_resp = self.client
                    .upload_part()
                    .bucket(&self.bucket)
                    .key(key)
                    .upload_id(upload_id)
                    .part_number(part_number)
                    .body(ByteStream::from(part_data))
                    .send()
                    .await?;

                completed_parts.push(
                    CompletedPart::builder()
                        .part_number(part_number)
                        .e_tag(part_resp.e_tag().unwrap().to_string())
                        .build(),
                );

                part_number += 1;
            }
        }

        // Final (possibly < 5MB) part
        if !buffer.is_empty() {
            let part_resp = self.client
                .upload_part()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(upload_id)
                .part_number(part_number)
                .body(ByteStream::from(buffer.clone()))
                .send()
                .await?;

            completed_parts.push(
                CompletedPart::builder()
                    .part_number(part_number)
                    .e_tag(part_resp.e_tag().unwrap().to_string())
                    .build(),
            );
        }

        if completed_parts.is_empty() {
            return Err(anyhow::anyhow!("No data to upload"));
        }

        let completed_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();

        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(upload_id)
            .multipart_upload(completed_upload)
            .send()
            .await?;

        Ok(())
    }

    async fn download_from_s3_streaming(&self, key: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        
        let response = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;

        let body = response.body;
        let reader = body.into_async_read();
        
        // Wrap the reader in a buffered reader with debug logging
        struct BufferedDebugReader<R> {
            inner: tokio::io::BufReader<R>,
            total_bytes: usize,
        }
        
        impl<R: AsyncRead + Unpin> AsyncRead for BufferedDebugReader<R> {
            fn poll_read(
                mut self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                let before = buf.filled().len();
                let result = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
                let after = buf.filled().len();
                let bytes_read = after - before;
                
                if bytes_read > 0 {
                    self.total_bytes += bytes_read;
                }
                
                result
            }
        }
        
        // Use a 1MB buffer for the reader
        let buffered_reader = tokio::io::BufReader::with_capacity(1024 * 1024, reader);
        
        Ok(Box::new(buffered_reader))
    }

}

impl S3Client {
    pub fn new(
        s3_config: S3Config,
    ) -> Self {
        let region = Region::new(s3_config.s3_region);
        
        let credentials = Credentials::new(
            &s3_config.aws_access_key_id,
            &s3_config.aws_secret_access_key,
            None,
            None,
            "rusty-pg-vault",
        );

        let mut config_builder = Builder::new()
            .region(region)
            .credentials_provider(credentials)
            .behavior_version(BehaviorVersion::latest());

        // Add endpoint if provided
        if !s3_config.aws_endpoint_url.is_empty() {
            config_builder = config_builder.endpoint_url(s3_config.aws_endpoint_url);
        }

        let config = config_builder.build();
        let client = Client::from_conf(config);

        Self { client, bucket: s3_config.s3_bucket.clone()}
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
