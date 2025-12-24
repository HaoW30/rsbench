-- Simple Point Select Workload
-- Demonstrates basic Lua workload structure

function prepare(ctx)
    -- Create test table
    ctx.database:execute([[
        CREATE TABLE IF NOT EXISTS test_table (
            id INT PRIMARY KEY,
            value VARCHAR(255)
        )
    ]])

    -- Insert test data
    for i = 1, ctx.table_size do
        ctx.database:execute(
            "INSERT INTO test_table (id, value) VALUES (?, ?)",
            i,
            "test_value_" .. i
        )
    end
end

function next_operation(ctx)
    -- Generate random ID within table range
    local id = math.random(1, 10000)

    return {
        name = "point_select",
        sql = "SELECT * FROM test_table WHERE id = ?",
        params = {id},
        operation_type = "read"
    }
end

function cleanup()
    -- Optional cleanup
    -- ctx.database:execute("DROP TABLE IF EXISTS test_table")
end
