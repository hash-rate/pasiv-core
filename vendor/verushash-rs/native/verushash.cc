#include <stdint.h>
#include "crypto/verus_hash.h"

static void ensure_initialized()
{
    static bool once = [](){
        CVerusHash::init();
        CVerusHashV2::init();
        return true;
    }();
    (void)once;
}

extern "C" {

void verus_v1_hash(void *result, const void *data, size_t len)
{
    ensure_initialized();
    verus_hash(result, data, len);
}

void verus_v2_hash(void *result, const void *data, size_t len)
{
    ensure_initialized();
    thread_local CVerusHashV2 vh(SOLUTION_VERUSHHASH_V2);

    vh.Reset();
    vh.Write((const unsigned char*)data, len);
    vh.Finalize((unsigned char*)result);
}

void verus_v2_1_hash(void *result, const void *data, size_t len)
{
    ensure_initialized();
    thread_local CVerusHashV2 vh(SOLUTION_VERUSHHASH_V2_1);

    vh.Reset();
    vh.Write((const unsigned char*)data, len);
    vh.Finalize2b((unsigned char*)result);
}

void verus_v2_2_hash(void *result, const void *data, size_t len)
{
    ensure_initialized();
    thread_local CVerusHashV2 vh(SOLUTION_VERUSHHASH_V2_2);

    vh.Reset();
    vh.Write((const unsigned char*)data, len);
    vh.Finalize2b((unsigned char*)result);
}

}
