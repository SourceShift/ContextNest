#!/usr/bin/env python3
"""Prepare private server and application tenant credentials without printing secrets.

This does not start, restart, or migrate a service. Existing files are never
replaced; use a different directory for a new credential set.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import shlex

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--directory", type=Path, default=Path.home() / ".contextnest" / "tenant-auth")
parser.add_argument("--tenant", default="app", help="Application tenant identifier (default: app)")
args = parser.parse_args()
tenant_id = args.tenant
root = args.directory.expanduser().resolve()
root.mkdir(parents=True, exist_ok=True, mode=0o700)
os.chmod(root, 0o700)
tenant_env_name = f"CONTEXTNEST_{tenant_id.upper()}_TOKEN"
paths = {name: root / name for name in ["tenants.json", "server.env", f"{tenant_id}.env", "operator.headers"]}
if any(path.exists() for path in paths.values()):
    parser.error("credential files already exist; refusing to overwrite them")
operator_token, tenant_token = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
config = {"data_dir": str(root.parent / "tenant-data"), "operator_token_env": "CONTEXTNEST_OPERATOR_TOKEN", "tenants": [
    {"id": tenant_id, "token_env": tenant_env_name, "policy": {"version": 1, "kinds": ["conversation-turn"]}}
]}
contents = {
    "tenants.json": json.dumps(config, indent=2) + "\n",
    "operator.headers": f"Authorization: Bearer {operator_token}\n",
    "server.env": f"export CONTEXTNEST_OPERATOR_HEADERS={shlex.quote(str(paths['operator.headers']))}\n"
        f"export CONTEXTNEST_TENANTS_FILE={shlex.quote(str(paths['tenants.json']))}\n"
        f"export CONTEXTNEST_OPERATOR_TOKEN={shlex.quote(operator_token)}\n"
        f"export {tenant_env_name}={shlex.quote(tenant_token)}\n",
    f"{tenant_id}.env": f"export CONTEXTNEST_URL=http://127.0.0.1:28080\n"
        f"export CONTEXTNEST_TENANT={tenant_id}\n"
        f"export CONTEXTNEST_TENANT_TOKEN={shlex.quote(tenant_token)}\n",
}
for name, value in contents.items():
    descriptor = os.open(paths[name], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w") as stream:
        stream.write(value)
        stream.flush()
        os.fsync(stream.fileno())
for path in paths.values():
    print(path)
