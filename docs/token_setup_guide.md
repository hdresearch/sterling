# GitHub Repository and Scoped Token Setup

## Repository Created
- **Repository**: `yevbar/test-scoped-repo`
- **URL**: https://github.com/yevbar/test-scoped-repo
- **Visibility**: Public
- **Description**: Test repository for demonstrating scoped access tokens

## Creating a Fine-Grained Personal Access Token

Since the GitHub CLI doesn't directly support creating fine-grained personal access tokens, you'll need to create one manually through the GitHub web interface:

### Steps:
1. Go to GitHub Settings: https://github.com/settings/personal-access-tokens/fine-grained
2. Click "Generate new token"
3. Configure the token:
   - **Token name**: `test-scoped-repo-token`
   - **Description**: `Scoped token for test-scoped-repo with minimal permissions`
   - **Expiration**: Choose appropriate duration (e.g., 90 days)
   - **Resource owner**: Select your account (`yevbar`)
   - **Repository access**: Select "Selected repositories" and choose `yevbar/test-scoped-repo`

### Minimal Required Permissions:
For basic repository operations, grant these permissions:
- **Repository permissions**:
  - `Contents`: Read and write (for file operations)
  - `Metadata`: Read (for basic repository info)
  - `Pull requests`: Write (if you need to create/manage PRs)
  - `Issues`: Write (if you need to create/manage issues)

### Using the Token:
Once created, you can use the token with git or API calls:

```bash
# Clone with token
git clone https://[TOKEN]@github.com/yevbar/test-scoped-repo.git

# Use with gh CLI
export GITHUB_TOKEN=[YOUR_TOKEN]
gh repo view yevbar/test-scoped-repo

# Use with API calls
curl -H "Authorization: token [YOUR_TOKEN]" \
     https://api.github.com/repos/yevbar/test-scoped-repo
```

## Security Benefits:
- Token is scoped to only one repository
- Minimal permissions reduce attack surface
- Can be easily revoked if compromised
- Separate from your main GitHub authentication
