#!/usr/bin/env bash
# with-secrets.sh NAME=<secret id>... -- <command>...: reads each AWS Secrets
# Manager secret with the AWS credentials in this step's environment, masks
# it, drops the AWS credentials, and runs <command> with each secret as NAME.
#
# The credentials come from configure-aws-credentials with
# output-env-credentials: false, passed as step outputs to this one step, and
# nothing goes to $GITHUB_ENV: no other step of the job sees a secret or the
# credentials, and <command> sees only its secrets.
. "$(dirname "$0")/lib.sh"

for name in AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_SESSION_TOKEN; do
  [ -n "${!name:-}" ] || die "$name is not set; pass the configure-aws-credentials outputs to this step."
done
while [ $# -gt 0 ] && [ "$1" != -- ]; do
  name=${1%%=*} id=${1#*=}
  [[ "$name" =~ ^[A-Z][A-Z0-9_]*$ ]] && [ "$id" != "$1" ] && [ -n "$id" ] || die "Expected NAME=<secret id>, not $1."
  value=$(aws secretsmanager get-secret-value --region us-east-1 --secret-id "$id" \
    --query SecretString --output text) || die "Could not read $id."
  [ -n "$value" ] && [ "$value" != None ] || die "$id is empty."
  # Every line, so a PEM never shows in a log, whatever prints it.
  while IFS= read -r line; do
    [ -z "$line" ] || echo "::add-mask::$line"
  done <<< "$value"
  export "$name=$value"
  shift
done
[ "${1:-}" = -- ] && [ $# -gt 1 ] || die "Usage: with-secrets.sh NAME=<secret id>... -- <command>..."
shift
unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_SESSION_TOKEN value
exec "$@"
