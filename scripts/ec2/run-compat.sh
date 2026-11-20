#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run the EC2-client compatibility checks against ONE Machina endpoint and print a per-action pass/fail table.
#
#   export MACHINA_ENDPOINT=https://HOST:5093          # the controller base URL, no /ec2 suffix
#   export AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=...   # POST /api/v1/ec2/access-keys (admin)
#   [MACHINA_INSECURE=1]            self-signed certificate
#   [MACHINA_PROJECT_ID=<uuid>]     lets boto3 create the VPC and launch template (the aws provider cannot send it)
#   [MACHINA_EC2_VPC_ID=vpc-.. MACHINA_EC2_SUBNET_ID=subnet-..]   an existing VPC/subnet for the ASG script and Terraform
#   [MACHINA_ELB_INSTANCE=i-..]     a throwaway instance for boto3_elbv2.py (skipped without it)
#   [MACHINA_TF=terraform|tofu] [MACHINA_SKIP="smoke vpc asg elbv2 terraform"]
#   ./scripts/ec2/run-compat.sh
#
# Creates throwaway resources named machina-compat* only, talks to MACHINA_ENDPOINT and nothing else, and never hides a
# failure: every FAIL is in the table, terraform's errors are listed one by one, and the exit status is 1 if any row failed.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
: "${MACHINA_ENDPOINT:?set MACHINA_ENDPOINT=https://HOST:5093}"
: "${AWS_ACCESS_KEY_ID:?set AWS_ACCESS_KEY_ID}"
: "${AWS_SECRET_ACCESS_KEY:?set AWS_SECRET_ACCESS_KEY}"
base="${MACHINA_ENDPOINT%/}"
case "$base" in *amazonaws.com*) echo "refusing: $base is not a Machina endpoint" >&2; exit 2 ;; esac
unset AWS_ENDPOINT_URL AWS_PROFILE AWS_SESSION_TOKEN
export MACHINA_EC2_ENDPOINT="$base/ec2" MACHINA_AUTOSCALING_ENDPOINT="$base/autoscaling" MACHINA_ELB_ENDPOINT="$base/elbv2"
[ "${MACHINA_INSECURE:-0}" = 1 ] && export MACHINA_EC2_VERIFY=false
tf="${MACHINA_TF:-terraform}"
skip=" ${MACHINA_SKIP:-} "
want() { case "$skip" in *" $1 "*) return 1 ;; esac; }

work="$(mktemp -d)"; results="$work/results.tsv"; : >"$results"
trap 'rm -rf "$work"' EXIT
row() { printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >>"$results"; }   # status script action detail

# Scripts that print RESULT lines (the compat scripts) or plain "PASS name" / "FAIL name" lines (the older ones).
run_py() {
  local name="$1"; shift
  local out="$work/$name.out"
  "$@" >"$out" 2>&1; local rc=$?
  awk -F'\t' -v s="$name" '
    $1=="RESULT" { print $2 "\t" $3 "\t" $4 "\t" $5; n++; next }
    /^PASS / { sub(/^PASS /,""); print "PASS\t" s "\t" $0 "\t"; n++; next }
    /^FAIL / { sub(/^FAIL /,""); print "FAIL\t" s "\t" $0 "\t"; n++; next }
    END { if (n==0) print "NONE\t" s "\t\t" }' "$out" >>"$results"
  # nothing parsable, or a non-zero exit without a FAIL row (a crash, a refusal): record it with the script's last words
  local tab=$'\t'
  if grep -q "^NONE${tab}$name${tab}" "$results"; then
    grep -v "^NONE${tab}$name${tab}" "$results" >"$results.new"; mv "$results.new" "$results"
    row FAIL "$name" "(no output, exit $rc)" "$(tail -n 3 "$out" | tr '\n' ' ' | cut -c1-250)"
  elif [ $rc -ne 0 ] && ! grep -q "^FAIL${tab}$name${tab}" "$results"; then
    row FAIL "$name" "(exit $rc)" "$(tail -n 3 "$out" | tr '\n' ' ' | cut -c1-250)"
  fi
}

# ---- discover what this API reports ------------------------------------------------------------------------------
disc="$(python3 "$here/_compat.py" discover 2>&1)"; drc=$?
if [ $drc -ne 0 ]; then
  row FAIL discover "DescribeImages/InstanceTypes/AvailabilityZones" "$(echo "$disc" | tail -n 2 | tr '\n' ' ')"
else
  row PASS discover "DescribeImages/InstanceTypes/AvailabilityZones" "$(echo "$disc" | tr '\n' ' ')"
  eval "$(echo "$disc" | sed -n 's/^\(IMAGE\|ITYPE\|AZ\)=\(.*\)$/\1="\2"/p')"
  export MACHINA_EC2_IMAGE="$IMAGE" MACHINA_EC2_INSTANCE_TYPE="$ITYPE" MACHINA_EC2_AZ="$AZ"
fi

if [ $drc -eq 0 ]; then
  want smoke && run_py boto3_smoke python3 "$here/boto3_smoke.py"
  if want elbv2; then
    if [ -n "${MACHINA_ELB_INSTANCE:-}" ]; then run_py boto3_elbv2 python3 "$here/boto3_elbv2.py"
    else row SKIP boto3_elbv2 "(all)" "set MACHINA_ELB_INSTANCE to a throwaway instance id"; fi
  fi
  if want vpc; then
    if [ -n "${MACHINA_PROJECT_ID:-}" ]; then run_py boto3_compat_vpc python3 "$here/boto3_compat_vpc.py"
    else row SKIP boto3_compat_vpc "(all)" "set MACHINA_PROJECT_ID"; fi
  fi
  if want asg; then
    if [ -n "${MACHINA_EC2_SUBNET_ID:-}" ]; then MACHINA_SUBNET="$MACHINA_EC2_SUBNET_ID" run_py boto3_compat_asg python3 "$here/boto3_compat_asg.py"
    else row SKIP boto3_compat_asg "(all)" "set MACHINA_EC2_SUBNET_ID to a ready subnet"; fi
  fi
fi

# ---- terraform -----------------------------------------------------------------------------------------------------
if [ $drc -eq 0 ] && want terraform; then
  if ! command -v "$tf" >/dev/null 2>&1; then
    row SKIP terraform "(all)" "$tf not installed"
  else
    cp "$here"/terraform/*.tf "$work/"; tfdir="$work"
    args=(-var "endpoint=$base" -var "access_key=$AWS_ACCESS_KEY_ID" -var "secret_key=$AWS_SECRET_ACCESS_KEY"
          -var "ami=$IMAGE" -var "instance_type=$ITYPE" -var "availability_zone=$AZ" -input=false -no-color)
    [ "${MACHINA_INSECURE:-0}" = 1 ] && args+=(-var insecure=true)
    [ -n "${MACHINA_EC2_VPC_ID:-}" ] && args+=(-var "vpc_id=$MACHINA_EC2_VPC_ID")
    [ -n "${MACHINA_EC2_SUBNET_ID:-}" ] && args+=(-var "subnet_id=$MACHINA_EC2_SUBNET_ID")
    [ -n "${MACHINA_EC2_VPC_ID:-}" ] && args+=(-var create_igw=false)
    [ "${MACHINA_TF_LAUNCH_TEMPLATE:-0}" = 1 ] && args+=(-var try_launch_template=true)
    tfrun() { ( cd "$tfdir" && "$tf" "$@" ) >"$work/tf.$1.out" 2>&1; }

    if tfrun init -input=false -no-color; then row PASS terraform init ""; else row FAIL terraform init "$(tail -n 3 "$work/tf.init.out" | tr '\n' ' ')"; fi
    if [ -d "$tfdir/.terraform" ]; then
      if tfrun apply -auto-approve "${args[@]}"; then row PASS terraform apply ""
      else
        row FAIL terraform apply "see the Error lines below"
        # one row per failed resource: "Error: <message>" followed by "with <address>,"
        awk '/Error: /{ sub(/^.*Error: /,""); msg=$0 } /with [a-z_]+\./{ addr=$0; sub(/^.*with /,"",addr); sub(/,.*$/,"",addr); print "FAIL\tterraform\t" addr "\t" msg; msg="" }' \
          "$work/tf.apply.out" >>"$results"
      fi
      # a plan after an apply must be empty (exit 0); 2 = the API reports something other than what was written
      ( cd "$tfdir" && "$tf" plan -detailed-exitcode "${args[@]}" ) >"$work/tf.plan.out" 2>&1; prc=$?
      case $prc in
        0) row PASS terraform "plan is empty after apply" "" ;;
        2) row FAIL terraform "plan is empty after apply" "$(grep -E '^\s+[~+-] ' "$work/tf.plan.out" | head -n 8 | tr -s ' ' | tr '\n' ';' | cut -c1-250)" ;;
        *) row FAIL terraform "plan is empty after apply" "plan failed: $(grep -m1 'Error' "$work/tf.plan.out" | cut -c1-200)" ;;
      esac
      # destroy runs whether or not apply succeeded, so a half-applied state does not leave resources behind
      if tfrun destroy -auto-approve "${args[@]}"; then row PASS terraform destroy ""; else row FAIL terraform destroy "$(grep -m3 'Error' "$work/tf.destroy.out" | tr '\n' ' ' | cut -c1-250)"; fi
    fi
  fi
fi

# ---- table -------------------------------------------------------------------------------------------------------
printf '\n%-5s  %-20s  %-52s  %s\n' STATUS SCRIPT ACTION DETAIL
awk -F'\t' '{ printf "%-5s  %-20s  %-52.52s  %s\n", $1, $2, $3, $4 }' "$results"
pass=$(grep -c '^PASS' "$results"); fail=$(grep -c '^FAIL' "$results"); skipped=$(grep -c '^SKIP' "$results")
printf '\n%s passed, %s failed, %s skipped\n' "$pass" "$fail" "$skipped"
[ "$fail" -eq 0 ]
