# Abyss object-storage test environments

This directory provisions disposable storage for Abyss's live
`storage_contract` test. Each subdirectory is an independent Terraform root:

| Directory | Service exercised | Resources created |
| --- | --- | --- |
| `aws` | AWS S3 | One private S3 bucket |
| `r2` | Cloudflare R2 | One R2 bucket |
| `spaces` | DigitalOcean Spaces | One private Space |
| `b2` | Backblaze B2 S3-compatible API | One private B2 bucket |
| `wasabi` | Wasabi S3-compatible API | One private Wasabi bucket |
| `azure` | Azure Blob and ADLS Gen2 | One HNS storage account and two containers |
| `gcs` | Google Cloud Storage | One private GCS bucket |
| `local` | MinIO, Ceph RGW preset, and custom S3 preset | One Dockerized MinIO server and three buckets |

The local Ceph and custom entries are **compatibility tests backed by MinIO**.
They verify the Abyss preset configuration, addressing, and S3 operations; they
are not a real Ceph deployment. To certify a Ceph release, point the generated
Ceph connection shape at an externally managed RGW endpoint and rerun the same
contract.

Cloud resources can incur storage, request, egress, retention, and account
charges. Review every `terraform plan`, use a dedicated test account or project,
and run `terraform destroy` when finished. `force_destroy` is enabled by
default because the contract deliberately writes objects.

## Security model

The cloud roots do not create or output long-lived credentials. Bootstrap
credentials are read from each provider's normal CLI cache, profile, or
environment variables. Create a separate, bucket-scoped credential for Abyss
after Terraform creates the bucket whenever the provider supports it.

Do not put tokens, keys, passwords, downloaded JSON credentials, backend
credentials, or real account IDs in committed `.tf` files. The example
`terraform.tfvars.example` files contain non-secret placeholders only. Real
`*.tfvars`, `.env*`, state, plans, CLI configuration, credentials, key stores,
and local object data are ignored by the repository's `.gitignore`.

Terraform state and saved plans can contain secrets even when an input is
marked `sensitive`. Local state is plaintext. The `local` root necessarily
records the MinIO environment in state because Docker owns that container
configuration. Protect the directory with normal workstation permissions, or
configure an encrypted remote backend with locking and access control before
applying. Never commit or send a state or plan file. See HashiCorp's
[sensitive-data guidance][terraform-sensitive] and
[state guidance][terraform-state].

Environment variables are inherited when Terraform or Abyss starts. Updating
an export in another parent shell does not change an already running Abyss
process; restart it after changing credentials.

## Prerequisites

Terraform 1.10 or newer is required. On macOS:

```sh
brew tap hashicorp/tap
brew install hashicorp/tap/terraform
brew install awscli azure-cli
brew install --cask google-cloud-sdk
```

Install Docker Desktop for the local root. Provider-specific CLIs are optional
but useful:

```sh
brew install doctl b2-tools jq
brew install minio/stable/mc
```

For Linux and Windows, use the vendor installation pages linked in the
provider sections. Confirm the tools actually selected by your shell:

```sh
terraform version
aws --version
az version
gcloud version
docker version
```

The checked-in `.terraform.lock.hcl` files pin provider checksums. Run
`terraform init -upgrade` only as an intentional provider update, inspect the
lock-file diff, and validate all roots afterward.

## Common workflow

Run commands from exactly one root at a time:

```sh
cd terraform/aws
cp terraform.tfvars.example terraform.tfvars
terraform init
terraform fmt -check
terraform validate
terraform plan -out=contract.tfplan
terraform apply contract.tfplan
terraform output contract
```

`terraform.tfvars` and `contract.tfplan` are ignored. Prefer environment
variables or external credential stores for secrets; do not add secrets to the
tfvars file just because it is ignored.

### Add the generated connection to Abyss

Abyss stores non-secret connection metadata in:

```text
~/Library/Application Support/Abyss/connections.toml
```

Create the file with a single version header if it does not exist:

```sh
config_dir="$HOME/Library/Application Support/Abyss"
mkdir -p "$config_dir"
config_file="$config_dir/connections.toml"
test -e "$config_file" || printf 'version = 1\n' >"$config_file"
terraform output -raw abyss_connection_toml >>"$config_file"
```

Run this only once per root. If an ID such as `terraform-aws` already exists,
replace that block instead of appending a duplicate. The output contains only
provider, endpoint, region, profile, account/project, and bucket metadata—not
credentials. AWS-format secrets stay in `~/.aws/credentials`; Azure and Google
tokens stay in their CLI/ADC caches.

### Run the live contract

Get the URI from `terraform output contract`, then run the test from the
repository root:

```sh
cd ../..
ABYSS_CONTRACT_URI='s3://terraform-aws/BUCKET_FROM_OUTPUT' \
  cargo test --test storage_contract -- --nocapture
```

Use `gs://` for GCS, `az://` for Azure Blob, and `adls://` for ADLS Gen2. The
contract writes beneath a random directory and removes it on success, but its
target must still be disposable.

For a local MinIO contract, the test can bypass `connections.toml`:

```sh
cd terraform/local
contract_uri="$(terraform output -json contract | jq -r '.contracts.minio')"
cd ../..
ABYSS_CONTRACT_URI="$contract_uri" \
ABYSS_S3_ENDPOINT='http://127.0.0.1:9000' \
ABYSS_S3_REGION='us-east-1' \
AWS_ACCESS_KEY_ID="$TF_VAR_minio_root_user" \
AWS_SECRET_ACCESS_KEY="$TF_VAR_minio_root_password" \
  cargo test --test storage_contract -- --nocapture
```

To test the `ceph-rgw` and `custom` presets specifically, install the generated
connections and use `.contracts.ceph` or `.contracts.custom`; the endpoint-only
test shortcut always constructs a MinIO preset.

## AWS S3

AWS IAM Identity Center is preferred over permanent IAM user keys. Configure a
profile and obtain a short-lived session:

```sh
aws configure sso --profile abyss-aws-admin
aws sso login --profile abyss-aws-admin
export AWS_PROFILE=abyss-aws-admin
cd terraform/aws
terraform init
terraform apply
```

The identity needs permission to create, configure, empty, and delete the
generated bucket. In a dedicated test account, an administrator can grant a
purpose-built role. At minimum the Terraform identity needs the bucket
operations represented by this root (`CreateBucket`, `DeleteBucket`,
`GetBucket*`, `ListBucket`, `PutBucketPublicAccessBlock`,
`PutBucketOwnershipControls`, `PutLifecycleConfiguration`) and object
list/delete permissions for cleanup.

After apply, create an `abyss-aws` profile that assumes a bucket-scoped test
role or uses another short-lived SSO role:

```sh
aws configure sso --profile abyss-aws
aws sso login --profile abyss-aws
```

The test identity needs to list the bucket and read, write, copy, multipart,
and delete objects inside it. If your organization still requires an IAM
access key, create it in IAM, copy the secret once, and enter it without putting
the value in shell history:

```sh
printf 'Access key ID: '
IFS= read -r AWS_TEST_ACCESS_KEY
printf 'Secret access key: '
IFS= read -rs AWS_TEST_SECRET_KEY
printf '\n'
aws configure set aws_access_key_id "$AWS_TEST_ACCESS_KEY" --profile abyss-aws
aws configure set aws_secret_access_key "$AWS_TEST_SECRET_KEY" --profile abyss-aws
aws configure set region eu-north-1 --profile abyss-aws
unset AWS_TEST_ACCESS_KEY AWS_TEST_SECRET_KEY
```

Do not create root-account access keys. Revoke an IAM user key with
`aws iam delete-access-key` as soon as the test setup no longer needs it.

References: [Terraform installation][terraform-install],
[AWS CLI SSO][aws-sso], and [AWS access-key practices][aws-access-keys].

## Cloudflare R2

Two credential types are involved:

1. A Cloudflare management API token lets Terraform create the bucket.
2. An R2 S3 API token lets Abyss read and write objects.

In the Cloudflare dashboard, copy the Account ID. Under **My Profile → API
Tokens**, create a token from a custom template with Account / Workers R2
Storage / Edit for only the test account. Enter it for the current shell:

```sh
printf 'Cloudflare management API token: '
IFS= read -rs CLOUDFLARE_API_TOKEN
printf '\n'
export CLOUDFLARE_API_TOKEN
cd terraform/r2
cp terraform.tfvars.example terraform.tfvars
# Replace the placeholder cloudflare_account_id, then:
terraform init
terraform apply
```

After the bucket exists, go to **R2 → Overview → Manage R2 API Tokens**.
Create an **Object Read & Write** token scoped only to the generated bucket.
Copy its Access Key ID and Secret Access Key immediately; the secret is shown
only once. Save it in the AWS-format profile named by `abyss_profile`:

```sh
printf 'R2 access key ID: '
IFS= read -r R2_ACCESS_KEY
printf 'R2 secret access key: '
IFS= read -rs R2_SECRET_KEY
printf '\n'
aws configure set aws_access_key_id "$R2_ACCESS_KEY" --profile abyss-r2
aws configure set aws_secret_access_key "$R2_SECRET_KEY" --profile abyss-r2
aws configure set region auto --profile abyss-r2
unset R2_ACCESS_KEY R2_SECRET_KEY
```

The endpoint is
`https://ACCOUNT_ID.r2.cloudflarestorage.com`. If you deliberately use an EU
or FedRAMP jurisdiction bucket, override the generated endpoint because those
jurisdictions have distinct hostnames. Remove the management token from the
shell with `unset CLOUDFLARE_API_TOKEN` after apply.

References: [Cloudflare API tokens][cloudflare-api-token] and
[R2 S3 credentials][r2-token].

## DigitalOcean Spaces

Terraform needs both a DigitalOcean personal access token for the account API
and a Spaces key for the S3 API:

1. In **API → Tokens**, generate a short-lived personal access token with the
   scopes needed to create/delete Spaces.
2. In **Spaces Object Storage → Access Keys**, create an initial key that can
   create the Space.

```sh
printf 'DigitalOcean API token: '
IFS= read -rs DIGITALOCEAN_TOKEN
printf '\nSpaces access key ID: '
IFS= read -r SPACES_ACCESS_KEY_ID
printf 'Spaces secret key: '
IFS= read -rs SPACES_SECRET_ACCESS_KEY
printf '\n'
export DIGITALOCEAN_TOKEN SPACES_ACCESS_KEY_ID SPACES_SECRET_ACCESS_KEY
cd terraform/spaces
terraform init
terraform apply
```

After apply, create a separate Spaces access key with
**Read/Write/Delete** permission scoped to the generated Space, then save it:

```sh
printf 'Scoped Spaces access key ID: '
IFS= read -r SPACES_TEST_ACCESS_KEY
printf 'Scoped Spaces secret key: '
IFS= read -rs SPACES_TEST_SECRET_KEY
printf '\n'
aws configure set aws_access_key_id "$SPACES_TEST_ACCESS_KEY" --profile abyss-spaces
aws configure set aws_secret_access_key "$SPACES_TEST_SECRET_KEY" --profile abyss-spaces
aws configure set region ams3 --profile abyss-spaces
unset SPACES_TEST_ACCESS_KEY SPACES_TEST_SECRET_KEY
```

The S3 endpoint is `https://REGION.digitaloceanspaces.com`. Spaces access keys
are currently created in the control panel, not through the API or `doctl`.
Unset all three bootstrap variables after Terraform finishes and revoke keys
from the control panel when done.

References: [DigitalOcean API tokens][do-tokens] and
[Spaces access keys][spaces-keys].

## Backblaze B2

Enable B2 Cloud Storage, then create an application key in **Application Keys**.
The master application key is not supported by the S3-compatible API.

For Terraform bootstrap, the application key needs bucket-management
capabilities including `listBuckets`, `writeBuckets`, `deleteBuckets`, and
permissions necessary to configure lifecycle rules. Provide the key using the
Backblaze provider environment names:

```sh
printf 'B2 application key ID: '
IFS= read -r B2_APPLICATION_KEY_ID
printf 'B2 application key: '
IFS= read -rs B2_APPLICATION_KEY
printf '\n'
export B2_APPLICATION_KEY_ID B2_APPLICATION_KEY
cd terraform/b2
terraform init
terraform apply
```

Use the bucket details to copy the exact S3 region (for example
`eu-central-003`) into `terraform.tfvars` if the default differs.

After apply, create a key restricted to the generated bucket with
`listAllBucketNames`, `listBuckets`, `readBuckets`, `listFiles`, `readFiles`,
`writeFiles`, and `deleteFiles`. Both write and delete capabilities are needed
for S3 delete behavior. Map `keyID` to the AWS access-key ID and
`applicationKey` to the AWS secret:

```sh
printf 'Bucket-scoped B2 key ID: '
IFS= read -r B2_TEST_KEY_ID
printf 'Bucket-scoped B2 application key: '
IFS= read -rs B2_TEST_KEY
printf '\n'
aws configure set aws_access_key_id "$B2_TEST_KEY_ID" --profile abyss-b2
aws configure set aws_secret_access_key "$B2_TEST_KEY" --profile abyss-b2
aws configure set region eu-central-003 --profile abyss-b2
unset B2_TEST_KEY_ID B2_TEST_KEY
```

The endpoint is `https://s3.REGION.backblazeb2.com`. Unset the bootstrap
variables after apply and delete both application keys after destroy.

References: [B2 application keys][b2-app-keys] and
[B2 S3-compatible key capabilities][b2-s3-keys].

## Wasabi

Create a dedicated Wasabi sub-user, grant it only the test bucket-management
policy required by this root, and create an access key under **Access Keys**.
The secret is displayed only when the key is created.

Store bootstrap credentials in a separate AWS-format profile:

```sh
printf 'Wasabi access key ID: '
IFS= read -r WASABI_ACCESS_KEY
printf 'Wasabi secret key: '
IFS= read -rs WASABI_SECRET_KEY
printf '\n'
aws configure set aws_access_key_id "$WASABI_ACCESS_KEY" --profile abyss-wasabi-admin
aws configure set aws_secret_access_key "$WASABI_SECRET_KEY" --profile abyss-wasabi-admin
aws configure set region eu-central-2 --profile abyss-wasabi-admin
unset WASABI_ACCESS_KEY WASABI_SECRET_KEY

export AWS_PROFILE=abyss-wasabi-admin
cd terraform/wasabi
terraform init
terraform apply
```

Then create a separate `abyss-wasabi` sub-user/key with list, read, write,
multipart, copy, and delete permission limited to the generated bucket.
Wasabi endpoints vary by region; this root uses
`https://s3.REGION.wasabisys.com`. Check the Wasabi service URL table before
changing regions. Unset `AWS_PROFILE` and deactivate/delete keys after use.

References: [Wasabi access keys][wasabi-keys] and
[Wasabi service URLs][wasabi-urls].

## Azure Blob and ADLS Gen2

The root creates a hierarchical-namespace-enabled account and disables shared
access keys, so Abyss uses Microsoft Entra credentials for both Blob and ADLS.

For interactive development:

```sh
az login
az account list --output table
az account set --subscription 'SUBSCRIPTION_ID_OR_NAME'
export ARM_SUBSCRIPTION_ID="$(az account show --query id -o tsv)"
cd terraform/azure
terraform init
terraform apply
```

The Terraform identity needs Contributor-like management permission in the
target subscription or a narrower scope that allows resource groups, storage
accounts, and containers. Management roles do not automatically grant object
data access. After apply, assign the identity running Abyss **Storage Blob Data
Contributor** on the generated storage account:

```sh
account_id="$(terraform output -json contract | jq -r '.storage_account_id')"
principal_id="$(az ad signed-in-user show --query id -o tsv)"
az role assignment create \
  --assignee-object-id "$principal_id" \
  --assignee-principal-type User \
  --role 'Storage Blob Data Contributor' \
  --scope "$account_id"
export AZURE_STORAGE_ACCOUNT="$(terraform output -json contract | jq -r '.account')"
```

Role assignments can take several minutes to propagate. Restart Abyss after
setting `AZURE_STORAGE_ACCOUNT`; it uses the Azure CLI/developer-tools token
cache and never needs a storage account key.

For unattended testing, create a narrowly scoped service principal:

```sh
az ad sp create-for-rbac \
  --name abyss-storage-contract \
  --role Contributor \
  --scopes "/subscriptions/$ARM_SUBSCRIPTION_ID"
```

The command prints a client secret once. Store it in a secret manager. Set
`ARM_CLIENT_ID`, `ARM_CLIENT_SECRET`, `ARM_TENANT_ID`, and
`ARM_SUBSCRIPTION_ID` for Terraform. For Abyss's
`client-secret-environment` credential mode, set the corresponding
`AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, and `AZURE_TENANT_ID`, and change the
generated TOML credential value from `developer-tools` to
`client-secret-environment`. Grant the service principal Storage Blob Data
Contributor on the storage-account scope. Delete the service-principal
credential after the test.

Run both contracts:

```sh
blob_uri="$(terraform output -json contract | jq -r '.blob_contract_uri')"
adls_uri="$(terraform output -json contract | jq -r '.adls_contract_uri')"
cd ../..
ABYSS_CONTRACT_URI="$blob_uri" cargo test --test storage_contract -- --nocapture
ABYSS_CONTRACT_URI="$adls_uri" cargo test --test storage_contract -- --nocapture
```

References: [Azure CLI authentication][azure-login],
[Terraform service-principal environment][azure-terraform-sp],
and [Blob data RBAC][azure-blob-rbac].

## Google Cloud Storage

Choose a dedicated test project, enable billing, and install/initialize the
Google Cloud CLI. User ADC is simplest for local development:

```sh
gcloud auth login
gcloud config set project YOUR_PROJECT_ID
gcloud services enable storage.googleapis.com
gcloud auth application-default login
export TF_VAR_project_id=YOUR_PROJECT_ID
cd terraform/gcs
terraform init
terraform apply
```

The Terraform principal needs permissions to create, configure, empty, and
delete a bucket. For a dedicated test project, `roles/storage.admin` supplies
those permissions. After creation, grant the test principal only the required
bucket access (Storage Object Admin plus permissions to get/list the bucket),
or keep Storage Admin scoped to this disposable bucket:

```sh
bucket="$(terraform output -json contract | jq -r '.bucket')"
gcloud storage buckets add-iam-policy-binding "gs://$bucket" \
  --member='user:YOUR_EMAIL' \
  --role='roles/storage.admin'
```

Abyss and Terraform both use Application Default Credentials. Set
`GOOGLE_CLOUD_PROJECT` to make automatic source discovery explicit, then
restart Abyss:

```sh
export GOOGLE_CLOUD_PROJECT="$TF_VAR_project_id"
```

For automation, prefer Workload Identity Federation or service-account
impersonation over a downloaded key. If a legacy JSON key is unavoidable,
create it outside this repository, restrict the service account to the test
bucket, set `GOOGLE_APPLICATION_CREDENTIALS` to its absolute path, and delete
the key after testing. The repository ignores common JSON credential names,
but `.gitignore` is not a secret manager.

References: [Application Default Credentials][gcp-adc],
[Cloud Storage IAM roles][gcs-iam], and
[service-account key risks][gcp-service-account-keys].

## Local MinIO, Ceph preset, and custom S3

Start Docker Desktop first. Generate session-only local credentials; do not add
them to `terraform.tfvars`:

```sh
export TF_VAR_minio_root_user="abyss$(openssl rand -hex 6)"
export TF_VAR_minio_root_password="$(openssl rand -base64 32)"
cd terraform/local
terraform init
terraform apply
```

The local state contains these values because they are part of the Docker
container environment. Keep it local and protected. Configure Abyss's shared
AWS-format profile without exposing the values in command history:

```sh
aws configure set aws_access_key_id "$TF_VAR_minio_root_user" --profile abyss-local
aws configure set aws_secret_access_key "$TF_VAR_minio_root_password" --profile abyss-local
aws configure set region us-east-1 --profile abyss-local
```

The S3 API is `http://127.0.0.1:9000`; the MinIO console is
`http://127.0.0.1:9001`. The root creates:

- `terraform-minio`, using the `minio` preset;
- `terraform-ceph-compat`, using the `ceph-rgw` preset;
- `terraform-custom-compat`, using the `custom` preset.

All three use path-style addressing and separate buckets on the same MinIO
server. A real external Ceph RGW can use the same connection block with its
endpoint, region, profile, and bucket substituted:

```toml
[[connections]]
id = "ceph-live"
name = "External Ceph RGW"
provider = "s3"
preset = "ceph-rgw"
endpoint = "https://rgw.example.test"
region = "us-east-1"
profile = "ceph-live"
force_path_style = true
buckets = ["abyss-contract"]
```

Create an RGW user/key through the deployment's normal Ceph administration
workflow (for example, an administrator-controlled `radosgw-admin user
create`), store its access and secret keys in the `ceph-live` AWS profile, and
never paste the command response into this repository.

## Destroy and revoke

Destroy from the same root and state used for apply:

```sh
terraform plan -destroy -out=destroy.tfplan
terraform apply destroy.tfplan
```

For local MinIO, this removes the container, buckets, and Docker volume. The
downloaded image remains cached intentionally; remove it manually if desired.

After cloud destruction:

1. Verify the provider console shows no bucket, storage account, or resource
   group left behind.
2. Revoke/delete test access keys, application keys, API tokens, service
   principal credentials, and downloaded service-account keys.
3. Unset credential variables and close the shell.
4. Remove obsolete test profiles from `~/.aws/config` and
   `~/.aws/credentials`.
5. Remove the corresponding `terraform-*` connection block from
   `connections.toml`.

If `destroy` cannot empty a bucket, run the contract cleanup again or empty the
disposable bucket with the provider's CLI, then retry. Object retention,
versioning, legal holds, or recently propagated IAM can prevent immediate
deletion.

## Troubleshooting

- **Authentication succeeds in a CLI but not Abyss:** CLI login and ADC may be
  separate caches. Run the provider-specific login shown above, verify the
  profile named in `connections.toml`, and restart Abyss.
- **A source is unavailable:** press Enter on its source row to retry and read
  the full status error. A bucket-restricted S3 key may need the bucket listed
  in `buckets` because it cannot call `ListBuckets`.
- **Wrong S3 endpoint or signature region:** compare `terraform output
  contract` with the provider console. B2 region identifiers and Wasabi/Spaces
  regions are service-specific; R2 uses `auto`.
- **Azure returns 403:** Contributor is a management role, not a blob data
  role. Assign Storage Blob Data Contributor and wait for propagation.
- **GCS has two different users:** `gcloud auth login` authenticates the CLI;
  `gcloud auth application-default login` authenticates ADC clients.
- **Terraform starts before local MinIO is ready:** the root retries the MinIO
  health endpoint for roughly 30 seconds. Check `docker logs
  abyss-contract-minio` and verify ports 9000/9001 are free.
- **A provider asks to replace a bucket:** bucket names and locations are often
  immutable. Treat the root as disposable, review the replacement, and ensure
  no non-test data is present.

[terraform-install]: https://developer.hashicorp.com/terraform/install
[terraform-sensitive]: https://developer.hashicorp.com/terraform/language/manage-sensitive-data
[terraform-state]: https://developer.hashicorp.com/terraform/language/state
[aws-sso]: https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sso.html
[aws-access-keys]: https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_access-keys.html
[cloudflare-api-token]: https://developers.cloudflare.com/fundamentals/api/get-started/create-token/
[r2-token]: https://developers.cloudflare.com/r2/api/tokens/
[do-tokens]: https://docs.digitalocean.com/reference/api/create-personal-access-token/
[spaces-keys]: https://docs.digitalocean.com/products/spaces/how-to/manage-access/
[b2-app-keys]: https://www.backblaze.com/docs/cloud-storage-create-and-manage-app-keys
[b2-s3-keys]: https://www.backblaze.com/docs/cloud-storage-s3-compatible-app-keys
[wasabi-keys]: https://docs.wasabi.com/docs/creating-a-new-access-key
[wasabi-urls]: https://docs.wasabi.com/docs/service-urls-for-wasabis-storage-regions
[azure-login]: https://learn.microsoft.com/cli/azure/authenticate-azure-cli-interactively
[azure-terraform-sp]: https://learn.microsoft.com/azure/developer/terraform/authenticate-to-azure-with-service-principle
[azure-blob-rbac]: https://learn.microsoft.com/azure/storage/blobs/assign-azure-role-data-access
[gcp-adc]: https://cloud.google.com/docs/authentication/provide-credentials-adc
[gcs-iam]: https://cloud.google.com/storage/docs/access-control/iam-roles
[gcp-service-account-keys]: https://cloud.google.com/iam/docs/keys-create-delete
