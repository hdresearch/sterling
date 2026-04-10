use aws_sdk_s3::Client;

use crate::s3::ListObjectsError;

/// Lists all object keys in S3 with the given prefix, ie: a directory
pub async fn list_objects_with_prefix(
    client: &Client,
    bucket_name: &str,
    prefix: &str,
) -> Result<Vec<String>, ListObjectsError> {
    let mut object_keys = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let mut request = client.list_objects_v2().bucket(bucket_name).prefix(prefix);

        if let Some(token) = continuation_token.as_deref() {
            request = request.continuation_token(token);
        }

        let response = request.send().await?;

        if let Some(objects) = response.contents {
            object_keys.extend(objects.into_iter().filter_map(|object| object.key));
        }

        match response.is_truncated {
            Some(true) => {
                continuation_token = response.next_continuation_token;
                if continuation_token.is_none() {
                    break;
                }
            }
            _ => break,
        }
    }

    Ok(object_keys)
}
