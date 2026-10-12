-- lists.db schema v5: forgets the website "no cover" links (issue #233). A cover lookup while a
-- website was rate-limiting this server stored "no cover" for the title, trusted until the
-- revalidation period passed; looking these titles up again finds their covers. MangaBaka's links
-- and the website links with a cover are kept.
DELETE FROM cover_links WHERE source = 'website' AND url IS NULL;
