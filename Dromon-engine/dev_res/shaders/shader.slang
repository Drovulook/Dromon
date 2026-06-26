struct VSInput {
  float3 inPosition;
  float3 inColor;
  float2 inTexCoord;
  float3 inNormal;
}

// set 0, binding 0 : données « caméra », mises à jour une fois par frame.
struct UniformBuffer {
    float4x4 view;
    float4x4 proj;
    float4 lightDirection; // xyz = direction de propagation des rayons
    float4 lightColor;     // rgb = couleur, w = intensité
};
[[vk::binding(0, 0)]]
ConstantBuffer<UniformBuffer> ubo;

// Push constant : la matrice « model » propre à CHAQUE objet, poussée juste
// avant son draw.
struct PushConstants {
    float4x4 model;
    // Matrice normale = inverse-transposée de la 3x3 du model, calculée côté CPU.
    // Stockée en float4x4 (coin 3x3 utile), pour l'éclairage uniquement.
    float4x4 normalMatrix;
};
[[vk::push_constant]]
ConstantBuffer<PushConstants> pc;

struct VSOutput {
  float4 pos : SV_Position;
  float3 color;
  float2 fragTexCoord;
  float3 normal;
}

// set 1, binding 0 : la texture du matériau (un descriptor set par texture).
[[vk::binding(0, 1)]]
Sampler2D texture;

[shader("vertex")]
VSOutput vertMain(VSInput input) {
  VSOutput output;
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  output.pos = mul(ubo.proj, mul(ubo.view, worldPos));
  output.color = input.inColor;
  output.fragTexCoord = input.inTexCoord;

  output.normal = mul((float3x3)pc.normalMatrix, input.inNormal);
  return output;
}


[shader("fragment")]
float4 fragMain(VSOutput vertIn) : SV_Target {
  float3 baseColor = texture.Sample(vertIn.fragTexCoord).rgb;

  float3 N = normalize(vertIn.normal);
  float3 L = normalize(-ubo.lightDirection.xyz);

  // Loi de Lambert : éclairement proportionnel au cosinus de l'angle (N·L).
  float diffuse = max(dot(N, L), 0.0);

  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w; 
  float3 ambient = 0.05 * baseColor;                       
  float3 color = baseColor * lightCol * diffuse + ambient;

  return float4(color, 1.0);
}
