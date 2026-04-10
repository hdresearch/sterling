# To run this you need to:
# run: nix develop -c mkcert -install
# run: ./pg/scripts/insert-vers-tls.sh
# Have SNE running.


echo "Creating VM"
vm_id=$(./public-api.sh new --wait-boot | jq '.vm_id' -r)

echo "Created vm with id: $vm_id"

echo "Obtaining ssh key"

# Just don't ask...
#
# Okay fine.. It takes the ssh-key from the ssh-key endpoint and then formats it correctly
# so it can be written to disk.
echo -e $(./public-api.sh ssh-key $vm_id | jq '.ssh_private_key' | sed 's/\"//g') > test.pem
echo "Obtained ssh key"

chmod 600 ./test.pem

ssh -o HostKeyAlias="$vm_id.vm.vers.sh" -o ProxyCommand="openssl s_client -quiet -servername $vm_id.vm.vers.sh -connect localhost:443" -i ./test.pem root@localhost 'apt update && apt install nginx && systemctl enable nginx && systemctl start nginx'

sleep 2

rm test.pem

./public-api.sh vm-request-uuid $vm_id

echo "If you see nginx welcome screen tests passed!"

./public-api.sh delete $vm_id
