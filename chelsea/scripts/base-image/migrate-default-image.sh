# This script will rename the old `default` image to a new name before running create-default-image.sh as normal.

script_dir="$(cd "$(dirname "$0")" && pwd)"
create_default_image_sh="${script_dir}/create-default-image.sh"

migration_id=$(date +%Y%m%d%H%M%S)
old_name=default
new_name="default_${migration_id}"

echo "Renaming RBD image '$old_name' to '$new_name'"
rbd --id chelsea mv $old_name $new_name

$create_default_image_sh