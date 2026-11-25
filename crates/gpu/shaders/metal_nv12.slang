// NephClip Apple Silicon Metal-native biplanar NV12 / P010 zero-copy compositor shader.
// Authored in Slang for compilation to native Metal Shading Language (`slangc -target metal`).
// Samples directly from VideoToolbox IOSurface-backed CVMetalTextureCache Y + CbCr textures
// with zero CPU deinterleaving or pixel copies.

struct Uniforms {
    float4x4 transform;
    float4x4 yuvToRgb;
    float3 yuvOffset;
    float opacity;
    uint transferMode; // 0 = Rec.709, 1 = sRGB, 2 = PQ (HDR10), 3 = HLG
};

[[vk::binding(0, 0)]] ConstantBuffer<Uniforms> u;
[[vk::binding(1, 0)]] Texture2D<float> texY;      // R8Unorm (NV12) or R16Unorm (P010)
[[vk::binding(2, 0)]] Texture2D<float2> texCbCr;  // RG8Unorm (NV12) or RG16Unorm (P010)
[[vk::binding(3, 0)]] SamplerState linearSampler;

struct VsOut {
    float4 pos : SV_Position;
    float2 uv  : TEXCOORD0;
};

[shader("vertex")]
VsOut vs_main(uint vid : SV_VertexID) {
    static const float2 corners[4] = {
        float2(-1.0, -1.0),
        float2( 1.0, -1.0),
        float2(-1.0,  1.0),
        float2( 1.0,  1.0),
    };
    static const float2 uvs[4] = {
        float2(0.0, 1.0),
        float2(1.0, 1.0),
        float2(0.0, 0.0),
        float2(1.0, 0.0),
    };
    VsOut o;
    o.pos = mul(u.transform, float4(corners[vid], 0.0, 1.0));
    o.uv = uvs[vid];
    return o;
}

float3 eotf_rec709(float3 c) {
    float3 lo = c / 4.5;
    float3 hi = pow(max((c + 0.099) / 1.099, 0.0), 1.0 / 0.45);
    return select(c < 0.081, lo, hi);
}

[shader("fragment")]
float4 fs_nv12_zero_copy(VsOut i) : SV_Target {
    float y = texY.Sample(linearSampler, i.uv);
    float2 cbcr = texCbCr.Sample(linearSampler, i.uv);
    float3 yuv = float3(y, cbcr.x, cbcr.y) + u.yuvOffset;
    float3 rgb = saturate(mul((float3x3)u.yuvToRgb, yuv));
    float3 lin = (u.transferMode == 0) ? eotf_rec709(rgb) : rgb;
    // Premultiplied linear RGBA output for Apple Silicon Rgba16Float accumulator
    return float4(lin * u.opacity, u.opacity);
}
