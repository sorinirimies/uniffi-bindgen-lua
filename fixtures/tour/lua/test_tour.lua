-- End-to-end test of the generated Lua module against the `tour` fixture.
local tour = require("tour")

local checks, failures = 0, 0
local function check(cond, what)
    checks = checks + 1
    if not cond then
        failures = failures + 1
        io.stderr:write(debug.traceback("CHECK failed: " .. tostring(what), 2), "\n")
    end
end
local function raises(f, pattern)
    local ok, err = pcall(f)
    return not ok, err
end

-- Primitives
check(tour.add_u8(200, 100) == 44, "add_u8 wraps")
check(tour.add_i32(-5, 3) == -2, "add_i32")
check(tour.negate_i64(9) == -9, "negate_i64")
check(tour.half_f32(3.0) == 1.5, "half_f32")
check(tour.invert(true) == false, "invert")
check(tour.shout("héllo") == "HÉLLO", "shout")
check(tour.reverse_bytes("\1\2\3") == "\3\2\1", "reverse_bytes")
tour.nothing()
check(tour.greet("Ada", "Hello") == "Hello, Ada!", "greet")
local bad, err = raises(function() tour.add_u8(300, 1) end)
check(bad and tostring(err):find("out of range"), "u8 range: " .. tostring(err))
bad, err = raises(function() tour.shout(42) end)
check(bad and tostring(err):find("a string expected"), "type check: " .. tostring(err))

-- Containers
check(tour.maybe_double(4) == 8, "maybe_double")
check(tour.maybe_double(nil) == nil, "maybe_double nil")
check(tour.sum({1, 2, 3, 4}) == 10, "sum")
check(tour.sum({}) == 0, "sum empty")
local lengths = tour.word_lengths({"a", "bcd", "ef"})
check(lengths.bcd == 3 and lengths.a == 1, "word_lengths")
local nested = tour.nested({{"x", "y"}, {}})
check(nested[1][2] == "y" and #nested[2] == 0, "nested")
-- Regression: Lua arrays cannot hold nil, so a sequence of optionals carries `n`.
local holes = tour.nested({n = 3, nil, {"x", "y"}, {}})
check(holes.n == 3 and holes[1] == nil and holes[2][2] == "y" and #holes[3] == 0, "sequence of optionals keeps its nils")
local echoed = tour.echo_u64s({a = {1, 2}, b = {}})
check(echoed.a[2] == 2 and #echoed.b == 0, "echo_u64s")
check(tour.timestamp_plus(100, 2.5) == 102.5, "timestamp_plus")
check(tour.timestamp_plus(-10, 4.25) == -5.75, "timestamp before 1970")
check(tour.double_duration(1.5) == 3, "double_duration")

-- Records and enums
check(tour.mix("Red") == "Green", "mix")
bad, err = raises(function() tour.mix("Purple") end)
check(bad and tostring(err):find("not a variant"), "bad variant: " .. tostring(err))
local circle = tour.make_shape(0)
check(circle.tag == "Circle" and circle.radius == 2 and circle.center.y == 2, "make_shape circle")
local rect = tour.make_shape(1)
check(rect.tag == "Rect" and rect[2].x == 2, "make_shape rect (unnamed fields)")
check(tour.make_shape(2).tag == "Empty", "make_shape empty")
check(tour.area(circle) == 12 and tour.area(rect) == 6, "area")
check(tour.area({tag = "Circle", center = {x = 0, y = 0}, radius = 1}) == 3, "area literal")
-- Missing fields take their defaults.
check(tour.describe({name = "n", origin = {x = 7, y = 8}}) == "n:8:None::7", "describe defaults")
check(tour.describe({name = "n", retries = 2, label = "L", tags = {"a", "b"}, origin = {x = 7, y = 8}}) == 'n:2:Some("L"):a+b:7', "describe")
bad, err = raises(function() tour.describe({retries = 1}) end)
check(bad and tostring(err):find("field 'name' is missing"), "missing field: " .. tostring(err))

-- Errors
tour.fail_data(0)
bad, err = raises(function() tour.fail_data(1) end)
check(bad and err.tag == "BadCode" and err.code == 42, "BadCode")
bad, err = raises(function() tour.fail_data(2) end)
check(bad and err.tag == "TooLong" and err[1] == 9, "TooLong")
check(tostring(err) == "TooLong", "tostring(error)")
check(tour.fail_flat(0) == 7, "fail_flat ok")
bad, err = raises(function() tour.fail_flat(2) end)
check(bad and err.tag == "Denied" and err[1] == "nope", "Denied")
bad, err = raises(function() tour.fail_message(1) end)
check(bad and err.tag == "Io" and err.message == "io failure: disk", "flat error message")
check(tostring(err) == "Io: io failure: disk", "flat error tostring")
bad, err = raises(function() tour.panics() end)
check(bad and tostring(err):find("boom"), "panic: " .. tostring(err))

-- Objects
local counter = tour.Counter.new(10)
check(counter:get() == 10, "Counter.new")
check(counter:add(5) == 15, "add")
check(tour.Counter.zero():get() == 0, "named constructor")
bad, err = raises(function() tour.Counter.checked(-1) end)
check(bad and err.tag == "Denied", "throwing constructor")
local other = tour.Counter.new(100)
counter:merge(other)
check(counter:get() == 115 and other:get() == 100, "merge")
local twin = counter:twin()
check(twin == counter, "__eq from Rust's PartialEq")
check(tostring(counter) == "Counter(115)", "__tostring from Display")
check(counter:hash() == twin:hash(), "hash")
check(counter:maybe_twin(true):get() == 115 and counter:maybe_twin(false) == nil, "maybe_twin")
check(counter:try_sub(15) == 100, "try_sub")
bad, err = raises(function() counter:try_sub(1000) end)
check(bad and err.tag == "BadCode", "method error")
local holder = tour.make_holder(1)
check(holder.counter:get() == 1 and holder.others[2]:get() == 3, "objects in records")

-- Callback interfaces
local rec = {names = 0}
function rec:on_event(name, value) self.names = self.names + 1; self.last = name; return value * 10 end
function rec:on_done() self.done = true end
function rec:fallible(n)
    if n < 0 then error({tag = "Denied", "negative"}) end
    if n == 0 then error("plain failure") end
    return "ok" .. n
end
check(tour.notify(rec, 4) == 60, "notify")
check(rec.done and rec.names == 4 and rec.last == "event-3", "callbacks ran")
check(tour.try_listener(rec, 5) == "ok5", "try_listener")
bad, err = raises(function() tour.try_listener(rec, -1) end)
check(bad and err.tag == "Denied" and err[1] == "negative", "typed error from Lua")
bad, err = raises(function() tour.try_listener(rec, 0) end)
check(bad and tostring(err):find("plain failure"), "Lua error becomes a panic: " .. tostring(err))

-- Traits implemented in Rust and in Lua
local rust = tour.rust_greeter()
check(rust:greet("x") == "rust says hi to x", "rust greeter")
check(tour.use_greeter(rust, "y") == "rust says hi to y", "use_greeter rust")
local lua_greeter = tour.Greeter.from_impl({greet = function(self, name) return "lua says hi to " .. name end})
check(lua_greeter:greet("w") == "lua says hi to w", "Lua greeter from Lua")
check(tour.use_greeter(lua_greeter, "z") == "lua says hi to z", "Lua greeter from Rust")

-- Async
check(tour.slow_add(2, 3) == 5, "slow_add")
check(tour.slow_fail(false) == "fine", "slow_fail ok")
bad, err = raises(function() tour.slow_fail(true) end)
check(bad and err.tag == "Plain", "async error")
tour.slow_unit()
check(tour.Worker.new("w"):work(3) == "w did 3", "async method")
check(tour.fetch_via({fetch = function(self, key) return "fetched " .. key end}, "k") == "fetched k", "async callback")

-- Custom types
check(tour.next_handle(41) == 42, "custom integer")
check(tour.host_of("https://example.com/a/b") == "example.com", "custom string")

-- uniffi 0.32: boxes, sets, lent bytes
check(tour.eval({tag = "Add", left = {tag = "Neg", inner = {tag = "Num", value = 2}}, right = {tag = "Num", value = 5}}) == 3, "eval")
local s = tour.sum_expr({1, 2, 3, 4})
check(tour.eval(s) == 10 and s.right.value == 4, "sum_expr")
local leaf = {label = "leaf", children = {}}
check(tour.count_nodes({label = "root", children = {leaf, leaf}, first_sibling = leaf}) == 4, "count_nodes")
check(#tour.unique_words("a b a c b") == 3, "unique_words")
check(tour.set_sum({1, 2, 3}) == 6, "set_sum")
check(tour.checksum(string.rep("\1", 100000)) == 100000, "checksum")
local adler = tour.Adler.new()
adler:update("\1\2")
adler:update("\3")
check(adler:value() == (1 * 31 + 2) * 31 + 3, "Adler")

-- Garbage collection releases every handle (Rust's and the registry's).
do
    local g = tour.Greeter.from_impl({greet = function() return "" end})
    tour.use_greeter(g, "")
    local c = tour.Counter.new(1)
    c:add(1)
end
collectgarbage()
collectgarbage()

print(string.format("%d checks, %d failures", checks, failures))
os.exit(failures == 0 and 0 or 1)
