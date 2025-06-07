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

# Load environment variables from .env file
if [ -f .env ]; then
    echo -e "${YELLOW}Loading configuration from .env file...${NC}"
    export $(cat .env | grep -v '#' | xargs)
else
    echo -e "${RED}Warning: .env file not found. Using default configuration.${NC}"
    echo -e "${YELLOW}You can create a .env file based on .env.example${NC}"
fi

# Create a temporary directory for the deployment packages
DEPLOY_DIR="target/lambda-deploy"
rm -rf $DEPLOY_DIR
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
    
    # Create a zip file for deployment - use absolute paths to avoid issues
    cd $package_dir
    zip -j "$(pwd)/../$function_name.zip" bootstrap
    cd - > /dev/null
    
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

# Set AWS credentials from environment variables
if [ -n "$AWS_ACCESS_KEY_ID" ] && [ -n "$AWS_SECRET_ACCESS_KEY" ]; then
    echo -e "${YELLOW}Using AWS credentials from .env file...${NC}"
    export AWS_ACCESS_KEY_ID=$AWS_ACCESS_KEY_ID
    export AWS_SECRET_ACCESS_KEY=$AWS_SECRET_ACCESS_KEY
    export AWS_DEFAULT_REGION=${AWS_DEFAULT_REGION:-us-east-1}
    export AWS_DR_REGION=${AWS_DR_REGION:-us-west-2}
    PRIMARY_STACK_NAME=${PRIMARY_STACK_NAME:-dr-demo-primary}
    DR_STACK_NAME=${DR_STACK_NAME:-dr-demo-dr}
    DYNAMODB_APP_TABLE=${DYNAMODB_APP_TABLE:-dr-application-table}
    DYNAMODB_SENTINEL_TABLE=${DYNAMODB_SENTINEL_TABLE:-dr-sentinel-table}
else
    echo -e "${YELLOW}Using AWS credentials from environment or AWS CLI configuration...${NC}"
    AWS_DEFAULT_REGION=${AWS_DEFAULT_REGION:-us-east-1}
    AWS_DR_REGION=${AWS_DR_REGION:-us-west-2}
    PRIMARY_STACK_NAME=${PRIMARY_STACK_NAME:-dr-demo-primary}
    DR_STACK_NAME=${DR_STACK_NAME:-dr-demo-dr}
    DYNAMODB_APP_TABLE=${DYNAMODB_APP_TABLE:-dr-application-table}
    DYNAMODB_SENTINEL_TABLE=${DYNAMODB_SENTINEL_TABLE:-dr-sentinel-table}
fi

# Deploy Primary Region
echo "☁️ Deploying Primary Region (${AWS_DEFAULT_REGION})..."
sam deploy \
    --template-file cloudformation/primary-region.yaml \
    --stack-name ${PRIMARY_STACK_NAME} \
    --capabilities CAPABILITY_IAM \
    --region ${AWS_DEFAULT_REGION} \
    --resolve-s3

# Deploy DR Region
echo "☁️ Deploying DR Region (${AWS_DR_REGION})..."
sam deploy \
    --template-file cloudformation/dr-region.yaml \
    --stack-name ${DR_STACK_NAME} \
    --capabilities CAPABILITY_IAM \
    --region ${AWS_DR_REGION} \
    --resolve-s3

# Enable Global Tables
echo "🌐 Configuring DynamoDB Global Tables..."
aws dynamodb create-global-table \
    --global-table-name ${DYNAMODB_APP_TABLE} \
    --replication-group RegionName=${AWS_DEFAULT_REGION} RegionName=${AWS_DR_REGION} \
    --region ${AWS_DEFAULT_REGION} || true

aws dynamodb create-global-table \
    --global-table-name ${DYNAMODB_SENTINEL_TABLE} \
    --replication-group RegionName=${AWS_DEFAULT_REGION} RegionName=${AWS_DR_REGION} \
    --region ${AWS_DEFAULT_REGION} || true

echo "✅ Deployment complete!"
echo "📊 View CloudWatch Dashboard: https://console.aws.amazon.com/cloudwatch/"
echo "🔍 Test failover: ./scripts/test-failover.sh" 