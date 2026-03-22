#!/bin/bash
# ContextNest Deployment Automation Script
# Usage: ./deploy.sh [environment] [action]

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CONFIG_FILE="$PROJECT_ROOT/config/deploy/${1:-production}.yml"
ENVIRONMENT="${1:-production}"
ACTION="${2:-deploy}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging function
log() {
    echo -e "${BLUE}[$(date '+%Y-%m-%d %H:%M:%S')]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

# Load configuration
load_config() {
    if [[ -f "$CONFIG_FILE" ]]; then
        # Parse YAML using yq or simple parser
        if command -v yq &> /dev/null; then
            eval "$(yq eval '. | to_entries | .[] | "\(.key)=\(.value)"' "$CONFIG_FILE")"
        else
            warn "yq not found, using simple YAML parser"
            # Simple YAML parser (basic implementation)
            while IFS=': ' read -r key value; do
                [[ $key =~ ^[[:space:]]*# ]] && continue
                [[ -z $key ]] && continue
                key=$(echo "$key" | tr '[:lower:]' '[:upper:]' | tr '-' '_')
                value=$(echo "$value" | sed 's/^"//;s/"$//')
                eval "${key}=${value}"
            done < "$CONFIG_FILE"
        fi
        log "Loaded configuration for environment: $ENVIRONMENT"
    else
        error "Configuration file not found: $CONFIG_FILE"
    fi

    # Set defaults
    NAMESPACE="${NAMESPACE:-contextnest}"
    K8S_CONTEXT="${K8S_CONTEXT:-${ENVIRONMENT}}"
    REGISTRY="${REGISTRY:-ghcr.io/contextnest}"
    IMAGE_TAG="${IMAGE_TAG:-latest}"
    ROLLBACK_TIMEOUT="${ROLLBACK_TIMEOUT:-300}"
    HEALTH_CHECK_TIMEOUT="${HEALTH_CHECK_TIMEOUT:-300}"
}

# Check prerequisites
check_prerequisites() {
    log "Checking deployment prerequisites..."

    # Check if kubectl is available
    if ! command -v kubectl &> /dev/null; then
        error "kubectl is not installed or not in PATH"
    fi

    # Check if helm is available
    if ! command -v helm &> /dev/null; then
        warn "helm is not installed, some features may not work"
    fi

    # Check cluster connectivity
    if ! kubectl cluster-info --context "$K8S_CONTEXT" &> /dev/null; then
        error "Cannot connect to Kubernetes cluster with context: $K8S_CONTEXT"
    fi

    # Check if namespace exists
    if ! kubectl get namespace "$NAMESPACE" --context "$K8S_CONTEXT" &> /dev/null; then
        log "Namespace $NAMESPACE does not exist, creating it..."
        kubectl create namespace "$NAMESPACE" --context "$K8S_CONTEXT"
    fi

    # Check if required secrets exist
    local required_secrets=(
        "contextnest-secrets"
        "neo4j-credentials"
        "redis-credentials"
    )

    for secret in "${required_secrets[@]}"; do
        if ! kubectl get secret "$secret" -n "$NAMESPACE" --context "$K8S_CONTEXT" &> /dev/null; then
            warn "Secret $secret not found, deployment may fail"
        fi
    done

    success "Prerequisites check passed"
}

# Build Docker image
build_image() {
    log "Building Docker image..."

    local dockerfile="Dockerfile.production"
    local image_name="$REGISTRY/contextnest"
    local full_image_tag="$image_name:$IMAGE_TAG"

    log "Building image: $full_image_tag"

    # Build the image
    docker build \
        -f "$dockerfile" \
        -t "$full_image_tag" \
        --build-arg BUILD_DATE="$(date -u +'%Y-%m-%dT%H:%M:%SZ')" \
        --build-arg VERSION="$IMAGE_TAG" \
        --build-arg VCS_REF="$(git rev-parse HEAD)" \
        "$PROJECT_ROOT"

    # Push to registry
    log "Pushing image to registry..."
    docker push "$full_image_tag"

    success "Docker image built and pushed: $full_image_tag"
}

# Run pre-deployment tests
run_pre_deployment_tests() {
    log "Running pre-deployment tests..."

    # Run unit tests
    log "Running unit tests..."
    cargo test --lib

    # Run integration tests
    log "Running integration tests..."
    cargo test --test '*'

    # Run security scan
    if command -v trivy &> /dev/null; then
        log "Running security scan..."
        trivy image --severity HIGH,CRITICAL "$REGISTRY/contextnest:$IMAGE_TAG"
    fi

    success "Pre-deployment tests passed"
}

# Deploy application
deploy_application() {
    log "Deploying ContextNest application..."

    # Create backup before deployment
    log "Creating pre-deployment backup..."
    "$SCRIPT_DIR/backup.sh" backup

    # Update image tag in deployment
    log "Updating deployment with new image tag..."
    kubectl set image deployment/contextnest \
        contextnest="$REGISTRY/contextnest:$IMAGE_TAG" \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        --record

    # Wait for rollout to start
    sleep 10

    # Monitor rollout progress
    log "Monitoring deployment rollout..."
    kubectl rollout status deployment/contextnest \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        --timeout="$ROLLBACK_TIMEOUT"

    success "Application deployment completed"
}

# Run post-deployment verification
run_post_deployment_verification() {
    log "Running post-deployment verification..."

    # Wait for pods to be ready
    log "Waiting for pods to be ready..."
    kubectl wait --for=condition=ready pod \
        -l app=contextnest \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        --timeout="$HEALTH_CHECK_TIMEOUT"

    # Get service URL
    local service_url
    service_url=$(kubectl get ingress contextnest-ingress \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        -o jsonpath='{.spec.rules[0].host}')

    log "Service URL: https://$service_url"

    # Run health checks
    log "Running health checks..."
    local health_endpoints=(
        "/health"
        "/ready"
        "/api/version"
    )

    for endpoint in "${health_endpoints[@]}"; do
        log "Checking endpoint: $endpoint"
        if curl -f -s "https://$service_url$endpoint" > /dev/null; then
            success "Health check passed for $endpoint"
        else
            error "Health check failed for $endpoint"
        fi
    done

    # Run smoke tests
    log "Running smoke tests..."
    if [[ -f "$PROJECT_ROOT/tests/smoke.sh" ]]; then
        "$PROJECT_ROOT/tests/smoke.sh" "https://$service_url"
    else
        warn "Smoke tests not found, skipping"
    fi

    success "Post-deployment verification completed"
}

# Perform rollback if needed
rollback_deployment() {
    local previous_revision="${1:-}"

    log "Initiating deployment rollback..."

    if [[ -z "$previous_revision" ]]; then
        # Get previous revision
        previous_revision=$(kubectl rollout history deployment/contextnest \
            -n "$NAMESPACE" \
            --context "$K8S_CONTEXT" \
            | grep 'revision:' | tail -2 | head -1 | awk '{print $2}')
    fi

    log "Rolling back to revision: $previous_revision"

    # Perform rollback
    kubectl rollout undo deployment/contextnest \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        --to-revision="$previous_revision"

    # Wait for rollback to complete
    kubectl rollout status deployment/contextnest \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        --timeout="$ROLLBACK_TIMEOUT"

    success "Rollback completed to revision: $previous_revision"
}

# Scale deployment
scale_deployment() {
    local replicas="$1"

    log "Scaling deployment to $replicas replicas..."

    kubectl scale deployment/contextnest \
        --replicas="$replicas" \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT"

    kubectl rollout status deployment/contextnest \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        --timeout=300

    success "Deployment scaled to $replicas replicas"
}

# Get deployment status
get_deployment_status() {
    log "Getting deployment status..."

    # Get deployment information
    echo -e "\n${BLUE}Deployment Status:${NC}"
    kubectl get deployment contextnest \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT"

    # Get pod status
    echo -e "\n${BLUE}Pod Status:${NC}"
    kubectl get pods \
        -l app=contextnest \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT"

    # Get service status
    echo -e "\n${BLUE}Service Status:${NC}"
    kubectl get services \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT"

    # Get ingress status
    echo -e "\n${BLUE}Ingress Status:${NC}"
    kubectl get ingress \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT"

    # Get recent events
    echo -e "\n${BLUE}Recent Events:${NC}"
    kubectl get events \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        --sort-by='.lastTimestamp' \
        | tail -10
}

# Clean up old resources
cleanup_resources() {
    log "Cleaning up old resources..."

    # Remove old completed pods
    kubectl delete pods \
        -l app=contextnest \
        --field-selector=status.phase=Succeeded \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        2>/dev/null || true

    # Remove old failed pods
    kubectl delete pods \
        -l app=contextnest \
        --field-selector=status.phase=Failed \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        2>/dev/null || true

    # Clean up old replicasets
    kubectl delete replicasets \
        -l app=contextnest \
        -n "$NAMESPACE" \
        --context "$K8S_CONTEXT" \
        2>/dev/null || true

    success "Resource cleanup completed"
}

# Generate deployment report
generate_deployment_report() {
    local action="$1"
    local status="$2"

    local report_file="$PROJECT_ROOT/logs/deployment-${ENVIRONMENT}-${DATE}.log"
    mkdir -p "$(dirname "$report_file")"

    cat <<EOF > "$report_file"
===============================================
ContextNest Deployment Report
===============================================

Environment: $ENVIRONMENT
Action: $action
Status: $status
Timestamp: $(date)
Image Tag: $IMAGE_TAG
Namespace: $NAMESPACE
K8s Context: $K8S_CONTEXT

Deployment Details:
$(kubectl get deployment contextnest -n "$NAMESPACE" --context "$K8S_CONTEXT" -o yaml)

Service Details:
$(kubectl get service contextnest -n "$NAMESPACE" --context "$K8S_CONTEXT" -o yaml)

Recent Events:
$(kubectl get events -n "$NAMESPACE" --context "$K8S_CONTEXT" --sort-by='.lastTimestamp' | tail -20)

Health Check Results:
$(curl -s "https://$(kubectl get ingress contextnest-ingress -n "$NAMESPACE" --context "$K8S_CONTEXT" -o jsonpath='{.spec.rules[0].host}')/health")

===============================================
EOF

    success "Deployment report generated: $report_file"
}

# Help function
show_help() {
    cat <<EOF
ContextNest Deployment Automation Script

Usage: $0 [ENVIRONMENT] [ACTION] [OPTIONS]

ENVIRONMENTS:
    development     Development environment
    staging         Staging environment
    production      Production environment (default)

ACTIONS:
    deploy          Deploy the application
    rollback        Rollback to previous version
    scale NUM       Scale deployment to NUM replicas
    status          Show deployment status
    cleanup         Clean up old resources
    build           Build Docker image only
    test            Run tests only
    help            Show this help message

OPTIONS:
    --image-tag TAG     Docker image tag to deploy
    --context NAME      Kubernetes context to use
    --namespace NAME    Kubernetes namespace
    --timeout SECONDS   Operation timeout
    --dry-run           Show what would be done

EXAMPLES:
    $0 production deploy
    $0 staging rollback
    $0 production scale 5
    $0 development build
    $0 production status

For support, contact: infrastructure@contextnest.ai
EOF
}

# Main function
main() {
    local environment="${1:-production}"
    local action="${2:-deploy}"
    local dry_run=false

    # Parse command line arguments
    shift 2
    while [[ $# -gt 0 ]]; do
        case $1 in
            --image-tag)
                IMAGE_TAG="$2"
                shift 2
                ;;
            --context)
                K8S_CONTEXT="$2"
                shift 2
                ;;
            --namespace)
                NAMESPACE="$2"
                shift 2
                ;;
            --timeout)
                ROLLBACK_TIMEOUT="$2"
                HEALTH_CHECK_TIMEOUT="$2"
                shift 2
                ;;
            --dry-run)
                dry_run=true
                shift
                ;;
            *)
                error "Unknown option: $1"
                ;;
        esac
    done

    # Set log file
    LOG_FILE="$PROJECT_ROOT/logs/deployment-${environment}.log"
    mkdir -p "$(dirname "$LOG_FILE")"

    # Load configuration
    load_config

    log "Starting deployment: $environment $action"

    case $action in
        deploy)
            check_prerequisites
            run_pre_deployment_tests
            build_image
            deploy_application
            run_post_deployment_verification
            cleanup_resources
            generate_deployment_report "deploy" "success"
            ;;
        rollback)
            check_prerequisites
            rollback_deployment
            run_post_deployment_verification
            generate_deployment_report "rollback" "success"
            ;;
        scale)
            check_prerequisites
            if [[ -z "${3:-}" ]]; then
                error "Number of replicas required for scale action"
            fi
            scale_deployment "$3"
            ;;
        status)
            check_prerequisites
            get_deployment_status
            ;;
        cleanup)
            check_prerequisites
            cleanup_resources
            ;;
        build)
            build_image
            ;;
        test)
            run_pre_deployment_tests
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            error "Unknown action: $action. Use '$0 help' for usage information."
            ;;
    esac

    success "Deployment operation completed: $action"
}

# Execute main function with all arguments
main "$@"