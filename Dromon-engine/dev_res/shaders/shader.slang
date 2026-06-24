struct VSInput {
  float3 inPosition;
  float3 inColor;
  float2 inTexCoord;
}

// set 0, binding 0 : données « caméra », mises à jour une fois par frame.
struct UniformBuffer {
    float4x4 view;
    float4x4 proj;
};
[[vk::binding(0, 0)]]
ConstantBuffer<UniformBuffer> ubo;

// Push constant : la matrice « model » propre à CHAQUE objet, poussée juste
// avant son draw (pas de descriptor, transfert direct dans le command buffer).
struct PushConstants {
    float4x4 model;
};
[[vk::push_constant]]
ConstantBuffer<PushConstants> pc;

struct VSOutput {
  float4 pos : SV_Position;
  float3 color;
  float2 fragTexCoord;
}

// set 1, binding 0 : la texture du matériau (un descriptor set par texture).
[[vk::binding(0, 1)]]
Sampler2D texture;

[shader("vertex")]
VSOutput vertMain(VSInput input) {
  VSOutput output;
  output.pos = mul(ubo.proj, mul(ubo.view, mul(pc.model, float4(input.inPosition, 1.0))));
  output.color = input.inColor;
  output.fragTexCoord = input.inTexCoord;
  return output;
}


[shader("fragment")]
float4 fragMain(VSOutput vertIn) : SV_Target {
  return texture.Sample(vertIn.fragTexCoord);
}
