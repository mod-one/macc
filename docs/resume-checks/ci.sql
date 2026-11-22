WITH report AS (SELECT :input -> '$.observation' AS body),
assessment AS (
    SELECT CASE
        WHEN body ->> '$.commit' IS NOT (:input ->> '$.commit') THEN 'Publish a report for the candidate commit'
        WHEN body ->> '$.environment' IS NOT (:input ->> '$.target') THEN 'Run CI in the required environment'
        WHEN json_type(body, '$.finished_at') IS NOT 'integer' OR
             body ->> '$.finished_at' NOT BETWEEN (:input ->> '$.now_unix') - 3600 AND (:input ->> '$.now_unix')
            THEN 'Refresh the report within the one-hour window'
        WHEN body ->> '$.result' IS NOT 'passed' THEN 'CI must pass for the candidate'
        ELSE NULL END AS missing FROM report
)
SELECT json_object('status', iif(missing IS NULL, 'ready', 'waiting'),
                   'detail', coalesce(missing, 'Candidate passed CI in the required environment within the last hour'))
FROM assessment
