local thread_counter = 0
local key_space = tonumber(os.getenv("BENCH_KEY_SPACE")) or 1
local seed = tonumber(os.getenv("BENCH_SEED")) or 1337

function setup(thread)
    thread:set("thread_id", thread_counter)
    thread_counter = thread_counter + 1
end

function init(args)
    math.randomseed(seed + thread_id * 9973)
    math.random()
    math.random()
end

request = function()
    local key = math.random(key_space)
    return wrk.format("GET", nil, { ["x-bench-key"] = tostring(key) })
end

done = function(summary, latency, requests)
    local duration_seconds = summary.duration / 1000000
    local requests_per_second = summary.requests / duration_seconds
    local errors = summary.errors

    io.write(string.format(
        "BENCHMARK_CSV,%d,%.2f,%d,%d,%d,%d,%d,%d,%d,%d\n",
        summary.requests,
        requests_per_second,
        latency:percentile(50.0),
        latency:percentile(95.0),
        latency:percentile(99.0),
        errors.connect,
        errors.read,
        errors.write,
        errors.status,
        errors.timeout
    ))
end
