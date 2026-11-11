// Thin wrapper around NVIDIA's NVENC API.
// Input is a D3D11 NV12 (8-bit) or P010 (10-bit) texture that our own shader
// already converted with BT.709 colour, so NVENC never borrows the game's
// shader cores for colour conversion. Frames never leave video memory.

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d3d11.h>
#include <stdlib.h>
#include <string.h>
#include "nvEncodeAPI.h"

typedef NVENCSTATUS(NVENCAPI *PFN_Create)(NV_ENCODE_API_FUNCTION_LIST *);
typedef NVENCSTATUS(NVENCAPI *PFN_MaxVer)(uint32_t *);

#define MAX_REG 12
// Frames in flight. The caller must not overwrite a texture while it is one of
// the last DEPTH frames submitted.
#define DEPTH 3

typedef struct {
    void *tex;
    NV_ENC_REGISTERED_PTR reg;
} RegEntry;

typedef struct {
    NV_ENC_OUTPUT_PTR bitstream;
    NV_ENC_INPUT_PTR mapped;
} Slot;

typedef struct {
    HMODULE dll;
    NV_ENCODE_API_FUNCTION_LIST fn;
    void *enc;
    Slot slots[DEPTH];
    int head;    // next slot to submit into
    int pending; // submitted, not yet collected
    RegEntry regs[MAX_REG];
    int nregs;
    uint32_t width, height;
    NV_ENC_BUFFER_FORMAT fmt;
    uint64_t frame;
    unsigned char *out;
    uint32_t out_cap;
} Shim;

__declspec(dllexport) int nvs_depth(void) { return DEPTH; }

static void set_vui(NV_ENC_CONFIG_H264_VUI_PARAMETERS *v) {
    // BT.709, limited ("TV") range: what players assume for HD video.
    v->videoSignalTypePresentFlag = 1;
    v->videoFormat = NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED;
    v->videoFullRangeFlag = 0;
    v->colourDescriptionPresentFlag = 1;
    v->colourPrimaries = NV_ENC_VUI_COLOR_PRIMARIES_BT709;
    v->transferCharacteristics = NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709;
    v->colourMatrix = NV_ENC_VUI_MATRIX_COEFFS_BT709;
}

// codec: 0 = H.264, 1 = HEVC, 2 = AV1. ten_bit: input is P010 and the stream is
// 10-bit (HEVC/AV1 only). preset: 1..7.
// Returns 0 on success, otherwise an NVENCSTATUS (or negative for load errors).
__declspec(dllexport) int nvs_create(void *d3d11_device, uint32_t width, uint32_t height,
                                     uint32_t fps, int codec, int ten_bit, int preset_n, int aq,
                                     uint32_t cq, uint32_t max_kbps, void **out_shim) {
    *out_shim = NULL;
    if (codec == 0) ten_bit = 0;
    Shim *s = (Shim *)calloc(1, sizeof(Shim));
    if (!s) return -1;
    s->width = width;
    s->height = height;
    s->fmt = ten_bit ? NV_ENC_BUFFER_FORMAT_YUV420_10BIT : NV_ENC_BUFFER_FORMAT_NV12;

    s->dll = LoadLibraryA("nvEncodeAPI64.dll");
    if (!s->dll) { free(s); return -2; }
    PFN_Create create = (PFN_Create)GetProcAddress(s->dll, "NvEncodeAPICreateInstance");
    PFN_MaxVer maxver = (PFN_MaxVer)GetProcAddress(s->dll, "NvEncodeAPIGetMaxSupportedVersion");
    if (!create || !maxver) { FreeLibrary(s->dll); free(s); return -3; }

    uint32_t v = 0;
    maxver(&v);
    uint32_t need = (NVENCAPI_MAJOR_VERSION << 4) | NVENCAPI_MINOR_VERSION;
    if (v < need) { FreeLibrary(s->dll); free(s); return -4; } // driver too old

    s->fn.version = NV_ENCODE_API_FUNCTION_LIST_VER;
    NVENCSTATUS st = create(&s->fn);
    if (st != NV_ENC_SUCCESS) { FreeLibrary(s->dll); free(s); return st; }

    NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS op = {0};
    op.version = NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER;
    op.deviceType = NV_ENC_DEVICE_TYPE_DIRECTX;
    op.device = d3d11_device;
    op.apiVersion = NVENCAPI_VERSION;
    st = s->fn.nvEncOpenEncodeSessionEx(&op, &s->enc);
    if (st != NV_ENC_SUCCESS) { FreeLibrary(s->dll); free(s); return st; }

    GUID codec_guid = codec == 0 ? NV_ENC_CODEC_H264_GUID
                    : codec == 2 ? NV_ENC_CODEC_AV1_GUID
                                 : NV_ENC_CODEC_HEVC_GUID;
    static const GUID *presets[7] = {&NV_ENC_PRESET_P1_GUID, &NV_ENC_PRESET_P2_GUID, &NV_ENC_PRESET_P3_GUID,
                                     &NV_ENC_PRESET_P4_GUID, &NV_ENC_PRESET_P5_GUID, &NV_ENC_PRESET_P6_GUID,
                                     &NV_ENC_PRESET_P7_GUID};
    if (preset_n < 1) preset_n = 1;
    if (preset_n > 7) preset_n = 7;
    GUID preset = *presets[preset_n - 1];
    NV_ENC_TUNING_INFO tuning = NV_ENC_TUNING_INFO_HIGH_QUALITY;

    NV_ENC_PRESET_CONFIG pc = {0};
    pc.version = NV_ENC_PRESET_CONFIG_VER;
    pc.presetCfg.version = NV_ENC_CONFIG_VER;
    st = s->fn.nvEncGetEncodePresetConfigEx(s->enc, codec_guid, preset, tuning, &pc);
    if (st != NV_ENC_SUCCESS) goto fail;

    NV_ENC_CONFIG cfg = pc.presetCfg;
    cfg.version = NV_ENC_CONFIG_VER;
    // Keyframes are forced by the caller every 2 s of real time.
    cfg.gopLength = NVENC_INFINITE_GOPLENGTH;
    cfg.frameIntervalP = 1; // no B-frames: every frame comes out right away, simple muxing
    // Constant quality: VBR with a quality target and no average bitrate,
    // capped so fast motion can't explode the buffer size.
    cfg.rcParams.rateControlMode = NV_ENC_PARAMS_RC_VBR;
    cfg.rcParams.averageBitRate = 0;
    cfg.rcParams.maxBitRate = max_kbps * 1000;
    cfg.rcParams.vbvBufferSize = max_kbps * 1000 * 2;
    cfg.rcParams.targetQuality = (uint8_t)cq;
    cfg.rcParams.targetQualityLSB = 0;
    // Spatial AQ (OBS's strength 8). Temporal AQ, lookahead and multipass all
    // run on CUDA cores, i.e. they compete with the game, so they stay off.
    cfg.rcParams.enableAQ = aq ? 1 : 0;
    cfg.rcParams.aqStrength = aq ? 8 : 0;
    cfg.rcParams.enableTemporalAQ = 0;
    cfg.rcParams.enableLookahead = 0;
    cfg.rcParams.multiPass = NV_ENC_MULTI_PASS_DISABLED;

    if (codec == 0) {
        NV_ENC_CONFIG_H264 *h = &cfg.encodeCodecConfig.h264Config;
        h->idrPeriod = NVENC_INFINITE_GOPLENGTH;
        h->repeatSPSPPS = 1;
        set_vui(&h->h264VUIParameters);
        cfg.profileGUID = NV_ENC_H264_PROFILE_HIGH_GUID;
    } else if (codec == 2) {
        NV_ENC_CONFIG_AV1 *a = &cfg.encodeCodecConfig.av1Config;
        a->idrPeriod = NVENC_INFINITE_GOPLENGTH;
        a->repeatSeqHdr = 1;
        a->inputPixelBitDepthMinus8 = ten_bit ? 2 : 0;
        a->pixelBitDepthMinus8 = ten_bit ? 2 : 0;
        a->colorPrimaries = NV_ENC_VUI_COLOR_PRIMARIES_BT709;
        a->transferCharacteristics = NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709;
        a->matrixCoefficients = NV_ENC_VUI_MATRIX_COEFFS_BT709;
        a->colorRange = 0;
        cfg.profileGUID = NV_ENC_AV1_PROFILE_MAIN_GUID;
    } else {
        NV_ENC_CONFIG_HEVC *h = &cfg.encodeCodecConfig.hevcConfig;
        h->idrPeriod = NVENC_INFINITE_GOPLENGTH;
        h->repeatSPSPPS = 1;
        h->pixelBitDepthMinus8 = ten_bit ? 2 : 0;
        set_vui(&h->hevcVUIParameters);
        cfg.profileGUID = ten_bit ? NV_ENC_HEVC_PROFILE_MAIN10_GUID : NV_ENC_HEVC_PROFILE_MAIN_GUID;
    }

    NV_ENC_INITIALIZE_PARAMS ip = {0};
    ip.version = NV_ENC_INITIALIZE_PARAMS_VER;
    ip.encodeGUID = codec_guid;
    ip.presetGUID = preset;
    ip.tuningInfo = tuning;
    ip.encodeWidth = width;
    ip.encodeHeight = height;
    ip.darWidth = width;
    ip.darHeight = height;
    ip.maxEncodeWidth = width;
    ip.maxEncodeHeight = height;
    ip.frameRateNum = fps;
    ip.frameRateDen = 1;
    ip.enablePTD = 1;
    ip.encodeConfig = &cfg;
    st = s->fn.nvEncInitializeEncoder(s->enc, &ip);
    if (st != NV_ENC_SUCCESS) goto fail;

    for (int i = 0; i < DEPTH; i++) {
        NV_ENC_CREATE_BITSTREAM_BUFFER bb = {0};
        bb.version = NV_ENC_CREATE_BITSTREAM_BUFFER_VER;
        st = s->fn.nvEncCreateBitstreamBuffer(s->enc, &bb);
        if (st != NV_ENC_SUCCESS) goto fail;
        s->slots[i].bitstream = bb.bitstreamBuffer;
    }

    *out_shim = s;
    return 0;

fail:
    s->fn.nvEncDestroyEncoder(s->enc);
    FreeLibrary(s->dll);
    free(s);
    return st;
}

static NV_ENC_REGISTERED_PTR get_reg(Shim *s, void *tex) {
    for (int i = 0; i < s->nregs; i++)
        if (s->regs[i].tex == tex) return s->regs[i].reg;
    if (s->nregs >= MAX_REG) return NULL;
    NV_ENC_REGISTER_RESOURCE r = {0};
    r.version = NV_ENC_REGISTER_RESOURCE_VER;
    r.resourceType = NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX;
    r.width = s->width;
    r.height = s->height;
    r.pitch = 0;
    r.resourceToRegister = tex;
    r.bufferFormat = s->fmt;
    r.bufferUsage = NV_ENC_INPUT_IMAGE;
    if (s->fn.nvEncRegisterResource(s->enc, &r) != NV_ENC_SUCCESS) return NULL;
    s->regs[s->nregs].tex = tex;
    s->regs[s->nregs].reg = r.registeredResource;
    s->nregs++;
    return r.registeredResource;
}

// Waits for the oldest in-flight frame and copies its bitstream out.
static NVENCSTATUS collect(Shim *s, const unsigned char **data, uint32_t *size) {
    int idx = (s->head - s->pending + DEPTH) % DEPTH;
    Slot *sl = &s->slots[idx];
    NV_ENC_LOCK_BITSTREAM lk = {0};
    lk.version = NV_ENC_LOCK_BITSTREAM_VER;
    lk.outputBitstream = sl->bitstream;
    NVENCSTATUS st = s->fn.nvEncLockBitstream(s->enc, &lk);
    if (st == NV_ENC_SUCCESS) {
        if (lk.bitstreamSizeInBytes > s->out_cap) {
            free(s->out);
            s->out_cap = lk.bitstreamSizeInBytes * 2;
            s->out = (unsigned char *)malloc(s->out_cap);
            if (!s->out) s->out_cap = 0;
        }
        if (s->out) {
            memcpy(s->out, lk.bitstreamBufferPtr, lk.bitstreamSizeInBytes);
            *data = s->out;
            *size = lk.bitstreamSizeInBytes;
        }
        s->fn.nvEncUnlockBitstream(s->enc, sl->bitstream);
    }
    if (sl->mapped) {
        s->fn.nvEncUnmapInputResource(s->enc, sl->mapped);
        sl->mapped = NULL;
    }
    s->pending--;
    return st;
}

// Submits one frame. Output lags input by up to DEPTH-1 frames: when the
// pipeline is full, the oldest frame's bitstream is returned in *data/*size
// (valid until the next call); otherwise *size is 0.
__declspec(dllexport) int nvs_encode(void *shim, void *d3d11_texture, int force_idr,
                                     const unsigned char **data, uint32_t *size) {
    Shim *s = (Shim *)shim;
    *data = NULL;
    *size = 0;
    NV_ENC_REGISTERED_PTR reg = get_reg(s, d3d11_texture);
    if (!reg) return -10;

    NVENCSTATUS st;
    if (s->pending == DEPTH) {
        st = collect(s, data, size);
        if (st != NV_ENC_SUCCESS) return st;
    }

    Slot *sl = &s->slots[s->head];
    NV_ENC_MAP_INPUT_RESOURCE map = {0};
    map.version = NV_ENC_MAP_INPUT_RESOURCE_VER;
    map.registeredResource = reg;
    st = s->fn.nvEncMapInputResource(s->enc, &map);
    if (st != NV_ENC_SUCCESS) return st;
    sl->mapped = map.mappedResource;

    NV_ENC_PIC_PARAMS pic = {0};
    pic.version = NV_ENC_PIC_PARAMS_VER;
    pic.inputWidth = s->width;
    pic.inputHeight = s->height;
    pic.inputBuffer = map.mappedResource;
    pic.bufferFmt = map.mappedBufferFmt;
    pic.outputBitstream = sl->bitstream;
    pic.pictureStruct = NV_ENC_PIC_STRUCT_FRAME;
    pic.inputTimeStamp = s->frame++;
    if (force_idr) pic.encodePicFlags = NV_ENC_PIC_FLAG_FORCEIDR | NV_ENC_PIC_FLAG_OUTPUT_SPSPPS;

    st = s->fn.nvEncEncodePicture(s->enc, &pic);
    if (st != NV_ENC_SUCCESS) {
        s->fn.nvEncUnmapInputResource(s->enc, sl->mapped);
        sl->mapped = NULL;
        return st;
    }
    s->head = (s->head + 1) % DEPTH;
    s->pending++;
    return 0;
}

// Collects one still-pending frame (for flushing). Returns 1 if a frame was
// returned, 0 if nothing is pending, or a negative/NVENC error.
__declspec(dllexport) int nvs_drain(void *shim, const unsigned char **data, uint32_t *size) {
    Shim *s = (Shim *)shim;
    *data = NULL;
    *size = 0;
    if (s->pending == 0) return 0;
    NVENCSTATUS st = collect(s, data, size);
    return st == NV_ENC_SUCCESS ? 1 : -(int)st;
}

__declspec(dllexport) void nvs_destroy(void *shim) {
    Shim *s = (Shim *)shim;
    if (!s) return;
    const unsigned char *d;
    uint32_t n;
    while (s->pending > 0) collect(s, &d, &n);
    NV_ENC_PIC_PARAMS eos = {0};
    eos.version = NV_ENC_PIC_PARAMS_VER;
    eos.encodePicFlags = NV_ENC_PIC_FLAG_EOS;
    s->fn.nvEncEncodePicture(s->enc, &eos);
    for (int i = 0; i < s->nregs; i++) s->fn.nvEncUnregisterResource(s->enc, s->regs[i].reg);
    for (int i = 0; i < DEPTH; i++)
        if (s->slots[i].bitstream) s->fn.nvEncDestroyBitstreamBuffer(s->enc, s->slots[i].bitstream);
    s->fn.nvEncDestroyEncoder(s->enc);
    FreeLibrary(s->dll);
    free(s->out);
    free(s);
}
