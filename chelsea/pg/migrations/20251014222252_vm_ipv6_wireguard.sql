-- migrate:up

-- Migration: Add IPv6 WireGuard support for VMs
-- Date: 2025-10-15
-- Description: Prepares the database for VM WireGuard connectivity by:
--   1. Ensuring VMs use IPv6 addresses (required for WireGuard)
--   2. Adding helper functions for account-based IP allocation
--   3. Adding indexes for efficient IP lookups

-- ============================================================================
-- PART 1: VM IP Address Constraints
-- ============================================================================

-- Ensure all VM IPs are IPv6 (required for WireGuard)
-- Note: The 'ip' column already uses type 'inet' which supports both IPv4 and IPv6
ALTER TABLE vms ADD CONSTRAINT vms_ip_is_ipv6
  CHECK (family(ip) = 6);

-- Add documentation for the IP column
COMMENT ON COLUMN vms.ip IS
  'VM WireGuard IPv6 address. Must be allocated from the account''s network subnet '
  'using next_vm_ip(). Used for WireGuard peer configuration between orchestrator and VM.';

-- Add index for efficient VM lookups by IP address
CREATE INDEX IF NOT EXISTS idx_vms_ip ON vms(ip);

-- ============================================================================
-- PART 2: Helper Functions
-- ============================================================================

-- Helper function to get account_id from cluster_id
-- Used during VM creation to determine which account subnet to allocate from
CREATE OR REPLACE FUNCTION public.get_account_id_from_cluster(_cluster_id uuid)
 RETURNS uuid
 LANGUAGE plpgsql
AS $function$
DECLARE
  _account_id UUID;
BEGIN
  SELECT accounts.account_id INTO _account_id
  FROM clusters
  LEFT JOIN api_keys ON clusters.owner_id = api_keys.api_key_id
  LEFT JOIN organizations ON api_keys.org_id = organizations.org_id
  LEFT JOIN accounts ON organizations.account_id = accounts.account_id
  WHERE clusters.cluster_id = _cluster_id;

  IF _account_id IS NULL THEN
    RAISE EXCEPTION 'No account found for cluster %', _cluster_id;
  END IF;

  RETURN _account_id;
END
$function$;

COMMENT ON FUNCTION get_account_id_from_cluster(uuid) IS
  'Retrieves the account_id for a given cluster_id by following the '
  'cluster -> api_key -> organization -> account relationship. '
  'Raises an exception if no account is found.';

-- ============================================================================
-- PART 3: Validation
-- ============================================================================

-- Verify that next_vm_ip() function exists (should be from 1_networking.sql)
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_proc WHERE proname = 'next_vm_ip'
  ) THEN
    RAISE EXCEPTION 'next_vm_ip() function not found. Ensure 1_networking.sql has been applied.';
  END IF;
END
$$;

-- Verify that accounts.network column exists
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_name = 'accounts' AND column_name = 'network'
  ) THEN
    RAISE EXCEPTION 'accounts.network column not found. Ensure 1_networking.sql has been applied.';
  END IF;
END
$$;

-- ============================================================================
-- NOTES FOR IMPLEMENTERS
-- ============================================================================
--
-- This migration prepares the database for WireGuard VM connectivity.
-- After applying this migration, the orchestrator can:
--
--   1. Call get_account_id_from_cluster(cluster_id) to find which account
--   2. Call next_vm_ip(account_id) to allocate an IPv6 from that account's /64
--   3. Store the IPv6 in vms.ip when creating the VM
--
-- Example usage in orchestrator code:
--
--   let account_id = db.get_account_id_from_cluster(cluster_id).await?;
--   let vm_ip = db.vms().allocate_vm_ip(account_id).await?;
--   db.vms().insert(cluster_id, node_id, vm_ip, ...).await?;
--
-- IPv6 Addressing Scheme:
--   - Base: fd00:fe11:deed::/48
--   - Per-account: /64 subnets (65,536 accounts possible)
--   - Per-VM: Individual IPs within account subnet (~18 quintillion per account)
--
-- Design Decision: /64 Subnet Size per Account
--
--   We chose /64 subnets per account as a balance between account capacity
--   and VMs per account. Below is a comparison of different subnet sizes:
--
--   +-------------+----------------+-------------------+---------------------+
--   | Subnet Size | Max Accounts   | VMs per Account   | Total Possible VMs  |
--   +-------------+----------------+-------------------+---------------------+
--   | /64 (CHOSEN)| 65,536         | ~18 quintillion   | ~1.2 sextillion     |
--   | /72         | 16.7 million   | ~72 quadrillion   | ~1.2 sextillion     |
--   | /80         | 4.3 billion    | ~281 trillion     | ~1.2 sextillion     |
--   +-------------+----------------+-------------------+---------------------+
--
--   Rationale for /64:
--     - 65k accounts is sufficient for initial scale
--     - /64 is the standard IPv6 subnet size (RFC 4291 best practices)
--     - Easier to reason about and debug
--     - Can migrate to smaller subnets (e.g., /72) later if more accounts needed
--     - VMs per account (~18 quintillion) is effectively unlimited
--
--   Note: Total IPv6 space remains constant (1.2 sextillion IPs from our /48).
--   The subnet size only affects the tradeoff between number of accounts vs
--   VMs per account. Both numbers are astronomically large in practice.
--
-- ============================================================================



-- migrate:down
