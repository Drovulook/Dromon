// Shader des render objects (modèles 3D texturés).
// Les déclarations partagées (UBO caméra, push constants, shadow map,
// computeShadow) sont dans common.slang.
#include "common.slang"

struct VSInput {
  float3 inPosition;
  float3 inColor;
  float2 inTexCoord;
  float3 inNormal;
}

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
// Propre aux objets → reste hors de common.slang.
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
