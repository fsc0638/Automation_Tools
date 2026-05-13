-- Computes the effective ACL role a user holds on a project.
-- Priority: direct ownership > project_acl > org membership > workspace membership.
-- Returns NULL when the user has no access (user_can_access_project would return false).
CREATE OR REPLACE FUNCTION user_project_role(p_project_id UUID, p_user_id UUID)
RETURNS TEXT
LANGUAGE SQL
STABLE
AS $$
    SELECT CASE
        WHEN p.user_id = p_user_id THEN 'owner'
        ELSE (
            SELECT role FROM (
                SELECT pa.role, access_role_rank(pa.role) AS rnk
                FROM project_acl pa
                WHERE pa.project_id = p_project_id AND pa.user_id = p_user_id
                UNION ALL
                SELECT om.role, access_role_rank(om.role)
                FROM organization_members om
                WHERE om.organization_id = p.organization_id AND om.user_id = p_user_id
                UNION ALL
                SELECT wm.role, access_role_rank(wm.role)
                FROM workspace_members wm
                WHERE wm.workspace_id = p.workspace_id AND wm.user_id = p_user_id
            ) ranked
            ORDER BY rnk DESC
            LIMIT 1
        )
    END
    FROM projects p
    WHERE p.id = p_project_id
$$;
