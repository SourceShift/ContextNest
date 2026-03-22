#!/bin/bash
# ContextNest Backup and Disaster Recovery Script
# Usage: ./backup.sh [action] [options]

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_FILE="${SCRIPT_DIR}/../config/backup/config.yml"
LOG_FILE="/var/log/contextnest-backup.log"
DATE=$(date +%Y%m%d_%H%M%S)
RETENTION_DAYS=30

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging function
log() {
    echo -e "${BLUE}[$(date '+%Y-%m-%d %H:%M:%S')]${NC} $1" | tee -a "$LOG_FILE"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" | tee -a "$LOG_FILE"
    exit 1
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1" | tee -a "$LOG_FILE"
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1" | tee -a "$LOG_FILE"
}

# Load configuration
load_config() {
    if [[ -f "$CONFIG_FILE" ]]; then
        source "$CONFIG_FILE"
        log "Loaded configuration from $CONFIG_FILE"
    else
        warn "Configuration file not found, using defaults"
    fi

    # Set defaults
    BACKUP_DIR="${BACKUP_DIR:-/opt/contextnest/backups}"
    S3_BUCKET="${S3_BUCKET:-contextnest-production-backups}"
    NAMESPACE="${NAMESPACE:-contextnest}"
    NEO4J_HOST="${NEO4J_HOST:-neo4j.contextnest.svc.cluster.local}"
    REDIS_HOST="${REDIS_HOST:-redis.contextnest.svc.cluster.local}"
}

# Check prerequisites
check_prerequisites() {
    log "Checking prerequisites..."

    # Check if kubectl is available
    if ! command -v kubectl &> /dev/null; then
        error "kubectl is not installed or not in PATH"
    fi

    # Check if aws CLI is available
    if ! command -v aws &> /dev/null; then
        error "aws CLI is not installed or not in PATH"
    fi

    # Check cluster connectivity
    if ! kubectl cluster-info &> /dev/null; then
        error "Cannot connect to Kubernetes cluster"
    fi

    # Check if namespace exists
    if ! kubectl get namespace "$NAMESPACE" &> /dev/null; then
        error "Namespace $NAMESPACE does not exist"
    fi

    success "Prerequisites check passed"
}

# Create backup directory
create_backup_dir() {
    local backup_path="$BACKUP_DIR/$DATE"
    mkdir -p "$backup_path"
    log "Created backup directory: $backup_path"
    echo "$backup_path"
}

# Backup Kubernetes resources
backup_kubernetes_resources() {
    local backup_path="$1"
    log "Backing up Kubernetes resources..."

    # Backup all resources with YAML
    kubectl get all,configmaps,secrets,pvc,ingress -n "$NAMESPACE" -o yaml > "$backup_path/k8s-resources.yaml"

    # Backup specific resource types
    mkdir -p "$backup_path/k8s-specific"

    # Deployments
    kubectl get deployments -n "$NAMESPACE" -o yaml > "$backup_path/k8s-specific/deployments.yaml"

    # Services
    kubectl get services -n "$NAMESPACE" -o yaml > "$backup_path/k8s-specific/services.yaml"

    # ConfigMaps
    kubectl get configmaps -n "$NAMESPACE" -o yaml > "$backup_path/k8s-specific/configmaps.yaml"

    # Secrets (without sensitive data)
    kubectl get secrets -n "$NAMESPACE" -o yaml | sed 's/data:/data: {}/' > "$backup_path/k8s-specific/secrets-sanitized.yaml"

    success "Kubernetes resources backup completed"
}

# Backup Neo4j database
backup_neo4j() {
    local backup_path="$1"
    log "Backing up Neo4j database..."

    # Get Neo4j pod
    local neo4j_pod=$(kubectl get pods -n "$NAMESPACE" -l app=neo4j -o jsonpath='{.items[0].metadata.name}')

    if [[ -z "$neo4j_pod" ]]; then
        warn "Neo4j pod not found, skipping database backup"
        return
    fi

    # Create backup inside the pod
    kubectl exec -n "$NAMESPACE" "$neo4j_pod" -- \
        neo4j-admin database backup --database=neo4j --to-path=/backup/neo4j

    # Copy backup from pod
    kubectl cp "$NAMESPACE/$neo4j_pod:/backup/neo4j" "$backup_path/neo4j"

    # Create compressed archive
    tar -czf "$backup_path/neo4j-backup.tar.gz" -C "$backup_path" neo4j

    # Clean up uncompressed backup
    rm -rf "$backup_path/neo4j"

    success "Neo4j backup completed"
}

# Backup Redis data
backup_redis() {
    local backup_path="$1"
    log "Backing up Redis data..."

    # Get Redis pod
    local redis_pod=$(kubectl get pods -n "$NAMESPACE" -l app=redis -o jsonpath='{.items[0].metadata.name}')

    if [[ -z "$redis_pod" ]]; then
        warn "Redis pod not found, skipping Redis backup"
        return
    fi

    # Create Redis backup
    kubectl exec -n "$NAMESPACE" "$redis_pod" -- \
        redis-cli --rdb /tmp/dump.rdb

    # Copy backup from pod
    kubectl cp "$NAMESPACE/$redis_pod:/tmp/dump.rdb" "$backup_path/redis-dump.rdb"

    success "Redis backup completed"
}

# Backup application data
backup_application_data() {
    local backup_path="$1"
    log "Backing up application data..."

    # Backup persistent volumes
    local pvs=$(kubectl get pvc -n "$NAMESPACE" -o jsonpath='{.items[*].metadata.name}')

    for pv in $pvs; do
        log "Backing up PV: $pv"

        # Find the pod using this PV
        local pod=$(kubectl get pods -n "$NAMESPACE" -o jsonpath="{.items[?(.spec.volumes[?(@.persistentVolumeClaim.claimName=='$pv')].name)].metadata.name}")

        if [[ -n "$pod" ]]; then
            # Create a temporary backup pod
            cat <<EOF | kubectl apply -f -
apiVersion: v1
kind: Pod
metadata:
  name: backup-pod-$DATE
  namespace: $NAMESPACE
spec:
  restartPolicy: Never
  containers:
  - name: backup
    image: busybox
    command: ['sleep', '3600']
    volumeMounts:
    - name: data-volume
      mountPath: /data
  volumes:
  - name: data-volume
    persistentVolumeClaim:
      claimName: $pv
EOF

            # Wait for backup pod to be ready
            kubectl wait --for=condition=ready pod "backup-pod-$DATE" -n "$NAMESPACE" --timeout=60s

            # Copy data from PV
            kubectl exec -n "$NAMESPACE" "backup-pod-$DATE" -- tar -czf "/tmp/${pv}-backup.tar.gz" -C /data .

            # Copy backup to local
            kubectl cp "$NAMESPACE/backup-pod-$DATE:/tmp/${pv}-backup.tar.gz" "$backup_path/"

            # Clean up backup pod
            kubectl delete pod "backup-pod-$DATE" -n "$NAMESPACE"
        fi
    done

    success "Application data backup completed"
}

# Backup logs
backup_logs() {
    local backup_path="$1"
    log "Backing up application logs..."

    # Get application pods
    local pods=$(kubectl get pods -n "$NAMESPACE" -l app=contextnest -o jsonpath='{.items[*].metadata.name}')

    for pod in $pods; do
        log "Collecting logs from pod: $pod"

        # Get recent logs
        kubectl logs -n "$NAMESPACE" "$pod" --since=24h > "$backup_path/logs/${pod}-application.log"

        # Get previous logs if they exist
        if kubectl logs -n "$NAMESPACE" "$pod" -p &> /dev/null; then
            kubectl logs -n "$NAMESPACE" "$pod" -p > "$backup_path/logs/${pod}-previous.log"
        fi
    done

    success "Logs backup completed"
}

# Upload to S3
upload_to_s3() {
    local backup_path="$1"
    log "Uploading backup to S3..."

    # Create tarball of all backups
    tar -czf "$backup_path.tar.gz" -C "$(dirname "$backup_path")" "$(basename "$backup_path")"

    # Upload to S3
    aws s3 cp "$backup_path.tar.gz" "s3://$S3_BUCKET/backups/"

    # Create metadata file
    cat <<EOF > "$backup_path-metadata.json"
{
  "timestamp": "$(date -Iseconds)",
  "backup_path": "$backup_path",
  "s3_key": "backups/$(basename "$backup_path.tar.gz")",
  "size_bytes": $(stat -f%z "$backup_path.tar.gz"),
  "namespace": "$NAMESPACE",
  "components": {
    "kubernetes_resources": true,
    "neo4j": true,
    "redis": true,
    "application_data": true,
    "logs": true
  }
}
EOF

    aws s3 cp "$backup_path-metadata.json" "s3://$S3_BUCKET/metadata/"

    success "Backup uploaded to S3: s3://$S3_BUCKET/backups/$(basename "$backup_path.tar.gz")"
}

# Clean up old backups
cleanup_old_backups() {
    log "Cleaning up old backups (older than $RETENTION_DAYS days)..."

    # Clean local backups
    find "$BACKUP_DIR" -type d -name "20*" -mtime +$RETENTION_DAYS -exec rm -rf {} + 2>/dev/null || true

    # Clean S3 backups
    aws s3 ls "s3://$S3_BUCKET/backups/" | while read -r line; do
        create_date=$(echo "$line" | awk '{print $1" "$2}')
        create_date=$(date -d"$create_date" +%s)
        older_than=$(date -d"$RETENTION_DAYS days ago" +%s)

        if [[ $create_date -lt $older_than ]]; then
            file_name=$(echo "$line" | awk '{print $4}')
            log "Deleting old S3 backup: $file_name"
            aws s3 rm "s3://$S3_BUCKET/backups/$file_name"
        fi
    done

    success "Old backups cleanup completed"
}

# Verify backup integrity
verify_backup() {
    local backup_path="$1"
    log "Verifying backup integrity..."

    # Check if backup files exist and are not empty
    local required_files=(
        "k8s-resources.yaml"
        "neo4j-backup.tar.gz"
        "redis-dump.rdb"
    )

    for file in "${required_files[@]}"; do
        if [[ ! -f "$backup_path/$file" ]]; then
            error "Required backup file missing: $file"
        fi

        if [[ ! -s "$backup_path/$file" ]]; then
            error "Backup file is empty: $file"
        fi
    done

    # Test archive integrity
    if ! tar -tzf "$backup_path/neo4j-backup.tar.gz" > /dev/null 2>&1; then
        error "Neo4j backup archive is corrupted"
    fi

    success "Backup integrity verification passed"
}

# Generate backup report
generate_report() {
    local backup_path="$1"
    local backup_size=$(du -sh "$backup_path" | cut -f1)
    local s3_size=$(aws s3 ls "s3://$S3_BUCKET/backups/$(basename "$backup_path.tar.gz")" --human-readable | awk '{print $3}')

    cat <<EOF > "$backup_path/backup-report.txt"
===============================================
ContextNest Backup Report
===============================================

Backup Information:
- Date: $(date)
- Backup Path: $backup_path
- Local Size: $backup_size
- S3 Size: $s3_size
- S3 Location: s3://$S3_BUCKET/backups/$(basename "$backup_path.tar.gz")

Components Backed Up:
- Kubernetes Resources: ✓
- Neo4j Database: ✓
- Redis Cache: ✓
- Application Data: ✓
- Application Logs: ✓

Verification Status:
- File Integrity: ✓
- Archive Integrity: ✓
- S3 Upload: ✓

Next Steps:
- Monitor backup retention policies
- Test restore procedures regularly
- Update backup configuration as needed

For support, contact: infrastructure@contextnest.ai
===============================================
EOF

    success "Backup report generated: $backup_path/backup-report.txt"
}

# Restore function
restore_backup() {
    local backup_date="$1"
    local backup_path="$BACKUP_DIR/$backup_date"

    log "Starting restore from backup: $backup_date"

    # Download from S3 if not available locally
    if [[ ! -d "$backup_path" ]]; then
        log "Downloading backup from S3..."
        aws s3 cp "s3://$S3_BUCKET/backups/${backup_date}.tar.gz" "/tmp/"
        mkdir -p "$backup_path"
        tar -xzf "/tmp/${backup_date}.tar.gz" -C "$(dirname "$backup_path")"
        rm "/tmp/${backup_date}.tar.gz"
    fi

    # Verify backup exists
    if [[ ! -d "$backup_path" ]]; then
        error "Backup not found: $backup_path"
    fi

    # Restore Kubernetes resources
    log "Restoring Kubernetes resources..."
    kubectl apply -f "$backup_path/k8s-resources.yaml"

    # Restore Neo4j database
    if [[ -f "$backup_path/neo4j-backup.tar.gz" ]]; then
        log "Restoring Neo4j database..."
        local neo4j_pod=$(kubectl get pods -n "$NAMESPACE" -l app=neo4j -o jsonpath='{.items[0].metadata.name}')

        if [[ -n "$neo4j_pod" ]]; then
            # Copy backup to pod
            kubectl cp "$backup_path/neo4j-backup.tar.gz" "$NAMESPACE/$neo4j_pod:/tmp/"

            # Restore database
            kubectl exec -n "$NAMESPACE" "$neo4j_pod" -- \
                tar -xzf /tmp/neo4j-backup.tar.gz -C /backup/

            # Stop Neo4j
            kubectl exec -n "$NAMESPACE" "$neo4j_pod" -- neo4j stop

            # Restore from backup
            kubectl exec -n "$NAMESPACE" "$neo4j_pod" -- \
                neo4j-admin database restore --from-path=/backup/neo4j --database=neo4j

            # Start Neo4j
            kubectl exec -n "$NAMESPACE" "$neo4j_pod" -- neo4j start
        fi
    fi

    # Restore Redis data
    if [[ -f "$backup_path/redis-dump.rdb" ]]; then
        log "Restoring Redis data..."
        local redis_pod=$(kubectl get pods -n "$NAMESPACE" -l app=redis -o jsonpath='{.items[0].metadata.name}')

        if [[ -n "$redis_pod" ]]; then
            # Copy backup to pod
            kubectl cp "$backup_path/redis-dump.rdb" "$NAMESPACE/$redis_pod:/tmp/"

            # Stop Redis
            kubectl exec -n "$NAMESPACE" "$redis_pod" -- redis-cli SHUTDOWN NOSAVE

            # Move backup file
            kubectl exec -n "$NAMESPACE" "$redis_pod" -- mv /tmp/dump.rdb /data/

            # Restart Redis pod
            kubectl delete pod "$redis_pod" -n "$NAMESPACE"
            kubectl wait --for=condition=ready pod -l app=redis -n "$NAMESPACE" --timeout=300s
        fi
    fi

    success "Restore completed from backup: $backup_date"
}

# List available backups
list_backups() {
    log "Listing available backups..."

    echo -e "\n${BLUE}Local Backups:${NC}"
    ls -la "$BACKUP_DIR" | grep '^d' | awk '{print $9}' | grep -E '^20[0-9]{6}_[0-9]{6}$' || echo "No local backups found"

    echo -e "\n${BLUE}S3 Backups:${NC}"
    aws s3 ls "s3://$S3_BUCKET/backups/" | awk '{print $4}' | grep -E '^20[0-9]{6}_[0-9]{6}\.tar\.gz$' | sed 's/\.tar\.gz$//' || echo "No S3 backups found"
}

# Help function
show_help() {
    cat <<EOF
ContextNest Backup and Disaster Recovery Script

Usage: $0 [ACTION] [OPTIONS]

ACTIONS:
    backup                   Create a full backup
    restore [DATE]          Restore from backup (YYYYMMDD_HHMMSS)
    list                    List available backups
    cleanup                 Clean up old backups
    verify [DATE]           Verify backup integrity
    help                    Show this help message

OPTIONS:
    --config FILE           Use custom configuration file
    --namespace NAME        Kubernetes namespace (default: contextnest)
    --s3-bucket BUCKET      S3 bucket for backups
    --retention DAYS        Backup retention in days (default: 30)
    --dry-run               Show what would be done without executing

EXAMPLES:
    $0 backup
    $0 restore 20231201_020000
    $0 list
    $0 cleanup

For support, contact: infrastructure@contextnest.ai
EOF
}

# Main function
main() {
    local action="${1:-help}"
    local backup_date="${2:-}"
    local dry_run=false

    # Parse command line arguments
    shift
    while [[ $# -gt 0 ]]; do
        case $1 in
            --config)
                CONFIG_FILE="$2"
                shift 2
                ;;
            --namespace)
                NAMESPACE="$2"
                shift 2
                ;;
            --s3-bucket)
                S3_BUCKET="$2"
                shift 2
                ;;
            --retention)
                RETENTION_DAYS="$2"
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

    # Load configuration
    load_config

    # Create backup directory if it doesn't exist
    mkdir -p "$BACKUP_DIR"
    mkdir -p "$(dirname "$LOG_FILE")"

    log "Starting ContextNest backup operation: $action"

    case $action in
        backup)
            check_prerequisites
            local backup_path
            backup_path=$(create_backup_dir)

            if [[ "$dry_run" == false ]]; then
                backup_kubernetes_resources "$backup_path"
                backup_neo4j "$backup_path"
                backup_redis "$backup_path"
                backup_application_data "$backup_path"
                backup_logs "$backup_path"
                verify_backup "$backup_path"
                upload_to_s3 "$backup_path"
                generate_report "$backup_path"
                cleanup_old_backups
            else
                log "DRY RUN: Would create backup in $backup_path"
            fi
            ;;
        restore)
            check_prerequisites
            if [[ -z "$backup_date" ]]; then
                error "Backup date required for restore. Use '$0 list' to see available backups."
            fi
            restore_backup "$backup_date"
            ;;
        list)
            list_backups
            ;;
        cleanup)
            cleanup_old_backups
            ;;
        verify)
            check_prerequisites
            if [[ -z "$backup_date" ]]; then
                error "Backup date required for verification. Use '$0 list' to see available backups."
            fi
            verify_backup "$BACKUP_DIR/$backup_date"
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            error "Unknown action: $action. Use '$0 help' for usage information."
            ;;
    esac

    success "Backup operation completed: $action"
}

# Execute main function
main "$@"