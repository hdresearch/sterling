#!/bin/bash

set -e

echo "📚 Syncing documentation with hdresearch/vers-docs"

# Configuration
DOCS_REPO="hdresearch/vers-docs"
DOCS_PATH="docs/api/chelsea"  # Updated path structure
GENERATED_DOCS_DIR="generated/docs"
BRANCH_NAME="auto-update-chelsea-docs-$(date +%Y%m%d-%H%M%S)"

# Clone or update vers-docs repository
if [ ! -d "vers-docs" ]; then
    echo "🔄 Cloning vers-docs repository..."
    git clone "https://github.com/$DOCS_REPO.git" vers-docs
else
    echo "🔄 Updating vers-docs repository..."
    cd vers-docs
    git fetch origin
    git checkout main
    git pull origin main
    cd ..
fi

cd vers-docs

# Create a new branch for the documentation update
echo "🌿 Creating branch: $BRANCH_NAME"
git checkout -b "$BRANCH_NAME"

# Create the target directory if it doesn't exist
mkdir -p "$DOCS_PATH"

# Copy generated documentation
if [ -d "../$GENERATED_DOCS_DIR" ]; then
    echo "📋 Copying documentation files..."
    cp -r "../$GENERATED_DOCS_DIR"/* "$DOCS_PATH/"
    
    # Add changes to git
    git add "$DOCS_PATH"
    
    # Check if there are changes to commit
    if git diff --staged --quiet; then
        echo "ℹ️  No documentation changes to commit"
    else
        echo "💾 Committing documentation changes..."
        git commit -m "Auto-update Chelsea API documentation

Generated from hdresearch/chelsea OpenAPI specification
Timestamp: $(date -u +"%Y-%m-%d %H:%M:%S UTC")
Branch: $BRANCH_NAME"
        
        echo "🚀 Pushing changes..."
        git push origin "$BRANCH_NAME"
        
        echo "✅ Documentation updated successfully!"
        echo "   Repository: $DOCS_REPO"
        echo "   Branch: $BRANCH_NAME"
        echo ""
        echo "🔗 Create a pull request:"
        echo "   https://github.com/$DOCS_REPO/compare/main...$BRANCH_NAME"
        echo ""
        echo "💡 Or use GitHub CLI to automatically create a pull request:"
        echo "   gh pr create --title 'Auto-update Chelsea API docs' --body 'Generated from latest OpenAPI spec'"
    fi
else
    echo "⚠️  No generated documentation found at $GENERATED_DOCS_DIR"
    echo "   Make sure to run Sterling with --docs flag first"
fi

cd ..
