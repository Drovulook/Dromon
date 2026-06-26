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
    float4x4 lightViewProj; // view*proj de la lumière (projection orthographique)
    float4 lightDirection;  // xyz = direction de propagation des rayons
    float4 lightColor;      // rgb = couleur, w = intensité
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
  // Position du fragment dans l'espace de la lumière (avant division perspective).
  // Sert à échantillonner la shadow map dans le fragment shader.
  float4 shadowCoord;
}

// set 1, binding 0 : la texture du matériau (un descriptor set par texture).
[[vk::binding(0, 1)]]
Sampler2D texture;

// set 2, binding 0 : la shadow map. Sampler2DShadow = sampler de comparaison :
// SampleCmp(uv, ref) renvoie ~1.0 si ref <= profondeur stockée (fragment éclairé),
// ~0.0 sinon (fragment dans l'ombre), avec PCF 2×2 si filtrage linéaire.
[[vk::binding(0, 2)]]
Sampler2DShadow shadowMap;


[shader("vertex")]
VSOutput vertMain(VSInput input) {
  VSOutput output;
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  output.pos = mul(ubo.proj, mul(ubo.view, worldPos));
  output.color = input.inColor;
  output.fragTexCoord = input.inTexCoord;

  output.normal = mul((float3x3)pc.normalMatrix, input.inNormal);
  // Projection dans l'espace de la lumière (utilisée pour l'ombre).
  output.shadowCoord = mul(ubo.lightViewProj, worldPos);
  return output;
}


// Vertex shader « depth-only » de la passe d'ombre : on ne produit que la
// position projetée dans l'espace de la lumière. Aucun fragment shader associé.
[shader("vertex")]
float4 shadowVertMain(VSInput input) : SV_Position {
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  return mul(ubo.lightViewProj, worldPos);
}

// Renvoie un facteur d'éclairement ∈ [0,1] : 1 = pleinement éclairé, 0 = ombré.
float computeShadow(float4 shadowCoord, float NdotL) {
  // Division perspective (triviale en orthographique, où w = 1).
  float3 proj = shadowCoord.xyz / shadowCoord.w;

  // En dehors du frustum de la lumière (au-delà du far) → considéré éclairé.
  if (proj.z > 1.0) {
    return 1.0;
  }

  // NDC xy ∈ [-1,1] → coordonnées de texture uv ∈ [0,1].
  float2 uv = proj.xy * 0.5 + 0.5;

  // Biais dépendant de la pente : plus la surface est rasante par rapport à la
  // lumière (NdotL faible), plus on décale, pour limiter le « shadow acne ».
  float bias = max(0.0005, 0.0025 * (1.0 - NdotL));

  return shadowMap.SampleCmp(uv, proj.z - bias);
}

[shader("fragment")]
float4 fragMain(VSOutput vertIn) : SV_Target {
  float3 baseColor = texture.Sample(vertIn.fragTexCoord).rgb;

  float3 N = normalize(vertIn.normal);
  float3 L = normalize(-ubo.lightDirection.xyz);

  // Loi de Lambert : éclairement proportionnel au cosinus de l'angle (N·L).
  float NdotL = max(dot(N, L), 0.0);

  // Facteur d'ombre : la lumière directe est atténuée là où un objet occulte.
  float shadow = computeShadow(vertIn.shadowCoord, NdotL);

  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;
  float3 ambient = 0.05 * baseColor;
  // L'ambiant reste toujours présent → les zones d'ombre ne sont pas noires.
  float3 color = baseColor * lightCol * NdotL * shadow + ambient;

  return float4(color, 1.0);
}
