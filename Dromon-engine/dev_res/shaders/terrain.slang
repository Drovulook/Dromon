// Shader du terrain volumétrique (géométrie issue de marching cubes).

#include "common.slang"

// Entrées vertex : doivent matcher `TerrainVertex` côté Rust (locations 0,1).
struct VSInput {
  float3 inPosition;        // location 0
  float4 inMaterialWeights; // location 1 — poids des 4 matériaux dominants
}

struct VSOutput {
  float4 pos : SV_Position;
  float3 worldPos; // position monde, pour recalculer la normale dans le frag
  float4 materialWeights;
  float4 shadowCoord;
}

[shader("vertex")]
VSOutput terrainVertMain(VSInput input) {
  VSOutput output;
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  output.pos = mul(ubo.proj, mul(ubo.view, worldPos));
  output.worldPos = worldPos.xyz;
  output.materialWeights = input.inMaterialWeights;
  output.shadowCoord = mul(ubo.lightViewProj, worldPos);
  return output;
}

// Vertex shader « depth-only » de la passe d'ombre. Entrée minimale (position
// seule) → la pipeline d'ombre du terrain n'exige que la location 0.
struct ShadowVSInput {
  float3 inPosition; // location 0
}
[shader("vertex")]
float4 terrainShadowVertMain(ShadowVSInput input) : SV_Position {
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  return mul(ubo.lightViewProj, worldPos);
}

[shader("fragment")]
float4 terrainFragMain(VSOutput vertIn) : SV_Target {
  // Normale géométrique recalculée par face depuis les dérivées d'écran de la
  // position monde (flat shading). Évite de stocker une normale par vertex.
  // On l'oriente vers le haut (+Z) : le terrain heightmap n'a pas de surplombs.
  float3 N = normalize(cross(ddx(vertIn.worldPos), ddy(vertIn.worldPos)));
  if (N.z < 0.0) {
    N = -N;
  }

  // Couleur de base PROVISOIRE, pilotée par la pente : herbe sur les surfaces
  // planes, roche sur les pentes raides. Placeholder en attendant le vrai
  // texture splatting piloté par les matériaux (`materialWeights` + IDs).
  // N.z ≈ 1 → horizontal ; N.z ≈ 0 → vertical.
  float3 grass = float3(0.23, 0.42, 0.16);
  float3 rock = float3(0.38, 0.36, 0.33);
  float slope = 1.0 - saturate(N.z);
  float3 baseColor = lerp(grass, rock, smoothstep(0.35, 0.65, slope));

  float3 L = normalize(-ubo.lightDirection.xyz);
  float NdotL = max(dot(N, L), 0.0);

  float shadow = computeShadow(vertIn.shadowCoord, NdotL);
  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;
  float3 ambient = 0.05 * baseColor;
  float3 color = baseColor * lightCol * NdotL * shadow + ambient;

  return float4(color, 1.0);
}
