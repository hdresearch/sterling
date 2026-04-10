use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use aws_sdk_s3::Client;

use crate::s3::{client::get_s3_client, error::DeletePrefixError, list_objects_with_prefix};

pub async fn delete_objects(
    client: &Client,
    bucket_name: impl Into<String>,
    keys: impl IntoIterator<Item = impl Into<String>>,
) -> anyhow::Result<()> {
    let mut objects = Vec::new();
    for key in keys {
        objects.push(ObjectIdentifier::builder().key(key).build()?);
    }

    let to_delete = Delete::builder().set_objects(Some(objects)).build()?;

    client
        .delete_objects()
        .bucket(bucket_name)
        .delete(to_delete)
        .send()
        .await?;

    Ok(())
}

pub async fn delete_object(
    client: &Client,
    bucket_name: impl Into<String>,
    key: impl Into<String>,
) -> anyhow::Result<()> {
    client
        .delete_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?;

    Ok(())
}

/// Deletes all objects within the provided prefix. No-op if the prefix is empty.
pub async fn delete_prefix(bucket_name: &str, prefix: &str) -> Result<(), DeletePrefixError> {
    let client = get_s3_client().await;
    let keys = list_objects_with_prefix(client, bucket_name, prefix).await?;
    if keys.is_empty() {
        return Ok(());
    }

    delete_objects(client, bucket_name.to_string(), keys).await?;
    Ok(())
}
