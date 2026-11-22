WITH assessment AS (
    SELECT CASE
        WHEN :input ->> '$.observation.lock.kind' IS NOT 'missing'
            THEN 'Wait for the exporter to release its write lock'
        WHEN :input ->> '$.observation.digest' IS NOT (:input ->> '$.target')
            THEN 'Supply the dataset with the pinned SHA-256'
    END AS missing
)
SELECT json_object('status', iif(missing IS NULL, 'ready', 'waiting'),
                   'detail', coalesce(missing, 'The requested dataset is available and its lock has been released'))
FROM assessment
