SELECT json_object(
    'status', CASE WHEN EXISTS (
        SELECT 1 FROM json_each(:input, '$.observation') AS device
        WHERE device.value ->> '$.serial' = :input ->> '$.target'
          AND device.value ->> '$.state' = 'device'
    ) THEN 'ready' ELSE 'waiting' END,
    'detail', 'Reserved device ' || (:input ->> '$.target') || ' — authorized transports: ' ||
        coalesce((SELECT json_group_array(device.value ->> '$.serial')
                  FROM json_each(:input, '$.observation') AS device
                  WHERE device.value ->> '$.state' = 'device'), '[]')
)
