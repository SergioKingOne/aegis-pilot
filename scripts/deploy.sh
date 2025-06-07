#!/bin/bash
# scripts/deploy.sh

set -e

# Define colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Ensure the script is run from the project root
if [ ! -f "Cargo.toml" ]; then
    echo -e "${RED}Error: This script must be run from the project root directory${NC}"
    exit 1
fi

# Create a temporary directory for the deployment packages
DEPLOY_DIR="target/lambda-deploy"
mkdir -p $DEPLOY_DIR

# Function to build and package a Lambda function
package_lambda() {
    local function_name=$1
    local binary_name=$2
    
    echo -e "${YELLOW}Building ${function_name}...${NC}"
    
    # Build the binary with release profile
    cargo build --release --bin $binary_name
    
    # Create a directory for the deployment package
    local package_dir="$DEPLOY_DIR/$function_name"
    mkdir -p $package_dir
    
    # Copy the binary to the package directory and rename it to bootstrap
    cp "target/release/$binary_name" "$package_dir/bootstrap"
    
    # Create a zip file for deployment
    (cd $package_dir && zip -j "$DEPLOY_DIR/$function_name.zip" bootstrap)
    
    echo -e "${GREEN}Successfully packaged ${function_name} to ${DEPLOY_DIR}/${function_name}.zip${NC}"
}

# Build and package each Lambda function
echo -e "${YELLOW}Starting Lambda functions packaging...${NC}"

package_lambda "backup-manager" "backup-manager-bootstrap"
package_lambda "data-validator" "data-validator-bootstrap"
package_lambda "failover-controller" "failover-controller-bootstrap"
package_lambda "health-check" "health-check-bootstrap"

echo -e "${GREEN}All Lambda functions have been packaged for deployment!${NC}"
echo -e "${YELLOW}Deployment packages are available in ${DEPLOY_DIR}${NC}"

# Instructions for AWS CLI deployment
echo -e "\n${YELLOW}To deploy a function using AWS CLI, run:${NC}"
echo -e "aws lambda update-function-code --function-name FUNCTION_NAME --zip-file fileb://${DEPLOY_DIR}/FUNCTION_NAME.zip"
echo -e "\n${YELLOW}Example:${NC}"
echo -e "aws lambda update-function-code --function-name backup-manager --zip-file fileb://${DEPLOY_DIR}/backup-manager.zip"

# Deploy Primary Region
echo "☁️ Deploying Primary Region (us-east-1)..."
sam deploy \
    --template-file cloudformation/primary-region.yaml \
    --stack-name dr-demo-primary \
    --capabilities CAPABILITY_IAM \
    --region us-east-1 \
    --resolve-s3

# Deploy DR Region
echo "☁️ Deploying DR Region (us-west-2)..."
sam deploy \
    --template-file cloudformation/dr-region.yaml \
    --stack-name dr-demo-dr \
    --capabilities CAPABILITY_IAM \
    --region us-west-2 \
    --resolve-s3

# Enable Global Tables
echo "🌐 Configuring DynamoDB Global Tables..."
aws dynamodb create-global-table \
    --global-table-name dr-application-table \
    --replication-group RegionName=us-east-1 RegionName=us-west-2 \
    --region us-east-1 || true

aws dynamodb create-global-table \
    --global-table-name dr-sentinel-table \
    --replication-group RegionName=us-east-1 RegionName=us-west-2 \
    --region us-east-1 || true

echo "✅ Deployment complete!"
echo "📊 View CloudWatch Dashboard: https://console.aws.amazon.com/cloudwatch/"
echo "🔍 Test failover: ./scripts/test-failover.sh" 