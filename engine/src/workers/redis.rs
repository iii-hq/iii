// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::time::Duration;

pub const DEFAULT_REDIS_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

pub const JSON_UPDATE_SCRIPT: &str = r#"
    local json_decode = cjson.decode
    local json_encode = cjson.encode

    local key = KEYS[1]
    local field = ARGV[1]
    local ops_json = ARGV[2]

    local old_value_str = redis.call('HGET', key, field)
    local old_value = {}
    if old_value_str then
        local ok, decoded = pcall(json_decode, old_value_str)
        if ok then
            old_value = decoded
        else
            return {'false', 'failed to decode existing JSON: ' .. tostring(decoded)}
        end
    end

    local ops = json_decode(ops_json)
    local current = json_decode(json_encode(old_value))
    local using_missing_default = old_value_str == nil

    local function get_path(path)
        if path == nil then
            return nil
        end
        if type(path) == 'string' then
            return path
        end
        if type(path) == 'table' then
            if path[1] then
                return path[1]
            end
            if path['0'] then
                return path['0']
            end
        end
        return path
    end

    local function initial_append_value(value)
        if type(value) == 'string' then
            return value
        end
        return {value}
    end

    local function is_array(value)
        if type(value) ~= 'table' then
            return false
        end
        local max = 0
        local count = 0
        for k, _ in pairs(value) do
            if type(k) ~= 'number' or k < 1 or math.floor(k) ~= k then
                return false
            end
            if k > max then
                max = k
            end
            count = count + 1
        end
        return count > 0 and count == max
    end

    local function append_to_target(target, value)
        if target == nil or target == cjson.null then
            return true, initial_append_value(value)
        end
        if type(target) == 'string' then
            if type(value) == 'string' then
                return true, target .. value
            end
            return false, target
        end
        if is_array(target) then
            table.insert(target, value)
            return true, target
        end
        return false, target
    end

    for _, op in ipairs(ops) do
        if op.type == 'set' then
            local path = get_path(op.path)
            if (path == '' or path == nil) and op.value ~= nil then
                current = op.value
                using_missing_default = false
            else
                if type(current) ~= 'table' or current == nil then
                    current = {}
                end
                if op.value == nil then
                    current[path] = cjson.null
                else
                    current[path] = op.value
                end
                using_missing_default = false
            end
        elseif op.type == 'merge' then
            local path = get_path(op.path)
            if (path == nil or path == '') and type(current) == 'table' and type(op.value) == 'table' then
                for k, v in pairs(op.value) do
                    current[k] = v
                end
                using_missing_default = false
            end
        elseif op.type == 'increment' then
            local path = get_path(op.path)
            if type(current) ~= 'table' or current == nil then
                current = {}
            end
            local val = current[path]
            if type(val) == 'number' then
                current[path] = val + op.by
            else
                current[path] = op.by
            end
            using_missing_default = false
        elseif op.type == 'decrement' then
            local path = get_path(op.path)
            if type(current) ~= 'table' or current == nil then
                current = {}
            end
            local val = current[path]
            if type(val) == 'number' then
                current[path] = val - op.by
            else
                current[path] = -op.by
            end
            using_missing_default = false
        elseif op.type == 'append' then
            local path = get_path(op.path)
            if path == '' or path == nil then
                local changed, next_value = append_to_target(using_missing_default and cjson.null or current, op.value)
                if changed then
                    current = next_value
                    using_missing_default = false
                end
            else
                if type(current) == 'table' and current ~= nil then
                    local changed, next_value = append_to_target(current[path], op.value)
                    if changed then
                        current[path] = next_value
                        using_missing_default = false
                    end
                end
            end
        elseif op.type == 'remove' then
            local path = get_path(op.path)
            if type(current) == 'table' and current ~= nil then
                current[path] = nil
                using_missing_default = false
            end
        end
    end

    local new_value_str = json_encode(current)
    redis.call('HSET', key, field, new_value_str)

    return {'true', old_value_str or '', new_value_str}
"#;
