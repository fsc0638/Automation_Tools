-- Backfill: every user must have at least one Personal Org.
--
-- 0019 bootstrapped Personal Orgs only for users that already existed at
-- that time. Newly registered users have their Personal Org lazy-created
-- inside ensure_personal_org() (projects.rs:1091) — fine for typical flow,
-- but users who never create a project end up with zero org memberships.
--
-- That's a latent multi-tenant risk: every list endpoint joins through
-- organization_members, so an unbacked user is invisible to themselves
-- in cross-org features even though their own resources work. It also
-- complicates Phase-2 commercialization, where "user belongs to ≥ 1 org"
-- becomes an invariant we want to enforce in code.
--
-- Strategy: identical to 0019's bootstrap loop, but scoped to users with
-- zero memberships, so it's idempotent for current and future users with
-- only the "register but never project" pattern.

WITH unbacked_users AS (
    SELECT u.id, u.email, u.display_name
    FROM users u
    LEFT JOIN organization_members om ON om.user_id = u.id
    WHERE om.user_id IS NULL
),
new_orgs AS (
    INSERT INTO organizations (owner_user_id, name)
    SELECT
        uu.id,
        COALESCE(NULLIF(uu.display_name, ''), uu.email) || ' Personal Org'
    FROM unbacked_users uu
    RETURNING id, owner_user_id
)
INSERT INTO organization_members (organization_id, user_id, role)
SELECT id, owner_user_id, 'owner'
FROM new_orgs;

-- Default workspace per backfilled org.
INSERT INTO workspaces (organization_id, name)
SELECT o.id, 'Default Workspace'
FROM organizations o
LEFT JOIN workspaces w ON w.organization_id = o.id
WHERE w.id IS NULL;

INSERT INTO workspace_members (workspace_id, user_id, role)
SELECT w.id, o.owner_user_id, 'owner'
FROM workspaces w
JOIN organizations o ON o.id = w.organization_id
LEFT JOIN workspace_members wm
    ON wm.workspace_id = w.id AND wm.user_id = o.owner_user_id
WHERE wm.user_id IS NULL;
