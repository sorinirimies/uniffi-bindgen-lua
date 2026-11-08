/* Fuzzes the conversion of values coming from Rust into Lua, in the module
 * generated for the `tour` fixture: whatever the bytes, `from_wire` either marks
 * the reader failed or pushes a Lua value that `to_wire` turns back into exactly
 * the bytes it was read from. Run under ASan/UBSan by scripts/ci/fuzz_lua.nu,
 * with a pseudo-random driver (or libFuzzer when available). */
#include "tour.c"

#include <lualib.h>

static lua_State *fuzz_L;

typedef void (*from_fn)(lua_State *, uniffi_reader *);
typedef void (*to_fn)(lua_State *, int, uniffi_writer *, const char *);

typedef struct fuzz_case {
    const uint8_t *data;
    size_t len;
    from_fn from;
    to_fn to;
} fuzz_case;

static int fuzz_protected(lua_State *L) {
    fuzz_case *c = (fuzz_case *)lua_touserdata(L, 1);
    uniffi_reader r = uniffi_reader_new(c->data, c->len);
    c->from(L, &r);
    if (r.failed) {
        return 0;
    }
    uniffi_writer *w = uniffi_lua_writer(L);
    c->to(L, lua_gettop(L), w, "fuzz");
    if (w->len != r.pos || (w->len && memcmp(w->data, c->data, w->len) != 0)) {
        fputs("round trip changed the bytes\n", stderr);
        abort();
    }
    uniffi_writer_free(w);
    return 0;
}

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    if (size == 0) {
        return 0;
    }
    if (!fuzz_L) {
        fuzz_L = luaL_newstate();
        /* Error metatables and the dispatcher, as `require` would set them up. */
        luaopen_tour(fuzz_L);
        lua_pop(fuzz_L, 1);
    }
    static const struct {
        from_fn from;
        to_fn to;
    } kinds[] = {
        {tour_from_wire_Settings, tour_to_wire_Settings},
        {tour_from_wire_Shape, tour_to_wire_Shape},
        {tour_from_wire_Expr, tour_to_wire_Expr},
        {tour_from_wire_TreeNode, tour_to_wire_TreeNode},
        {tour_from_wire_SequenceOptionSequenceString, tour_to_wire_SequenceOptionSequenceString},
        {tour_from_wire_DataError, tour_to_wire_DataError},
    };
    fuzz_case c;
    c.data = data + 1;
    c.len = size - 1;
    c.from = kinds[data[0] % (sizeof kinds / sizeof kinds[0])].from;
    c.to = kinds[data[0] % (sizeof kinds / sizeof kinds[0])].to;
    lua_pushcfunction(fuzz_L, fuzz_protected);
    lua_pushlightuserdata(fuzz_L, &c);
    /* A value that does not convert back (e.g. a map with duplicate keys merged
     * by Lua) raises inside the pcall; only a different byte string aborts. */
    lua_pcall(fuzz_L, 1, 0, 0);
    lua_settop(fuzz_L, 0);
    return 0;
}

#ifdef UNIFFI_FUZZ_SMOKE
int main(int argc, char **argv) {
    unsigned long runs = argc > 1 ? strtoul(argv[1], NULL, 10) : 200000;
    uint64_t state = 7;
    uint8_t input[64];
    for (unsigned long i = 0; i < runs; ++i) {
        state = state * 6364136223846793005ull + 1442695040888963407ull;
        size_t len = (size_t)(state >> 58);
        for (size_t j = 0; j < len; ++j) {
            state = state * 6364136223846793005ull + 1442695040888963407ull;
            uint8_t byte = (uint8_t)(state >> 33);
            input[j] = (state >> 61) == 0 ? byte : (uint8_t)(byte % 4);
        }
        LLVMFuzzerTestOneInput(input, len);
    }
    lua_close(fuzz_L);
    printf("%lu inputs, no crash\n", runs);
    return 0;
}
#endif
