// Shader du terrain volumétrique (géométrie issue de Surface Nets).

#include "common.slang"

// Entrées vertex : doivent matcher `TerrainVertex` côté Rust (locations 0,1,2).
struct VSInput {
  float3 inPosition; // location 0
  float3 inNormal;   // location 1 — normale lissée (gradient de densité)
  float3 inColor;    // location 2 — couleur du sommet (matériaux mélangés côté CPU)
}

struct VSOutput {
  float4 pos : SV_Position;
  float3 worldNormal; // normale monde, interpolée pour l'éclairage lisse
  float3 color;       // couleur interpolée entre les sommets par le rasterizer
  float4 shadowCoord;
}

[shader("vertex")]
VSOutput terrainVertMain(VSInput input) {
  VSOutput output;
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  output.pos = mul(ubo.proj, mul(ubo.view, worldPos));
  // model = identité pour le terrain, mais on applique la normalMatrix par
  // cohérence (gère un éventuel scale non uniforme).
  output.worldNormal = mul((float3x3)pc.normalMatrix, input.inNormal);
  output.color = input.inColor;
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
  // Normale lissée interpolée depuis les sommets (gradient de densité). Donne un
  // ombrage doux sur la surface Surface Nets, contrairement au flat shading.
  // La renormaliser : l'interpolation linéaire raccourcit le vecteur.
  float3 N = normalize(vertIn.worldNormal);

  // Couleur du matériau : déjà calculée côté CPU (mélange des matériaux dominants
  // selon l'altitude) et interpolée par le rasterizer entre les sommets.
  float3 material = vertIn.color;

  float3 L = normalize(-ubo.lightDirection.xyz);
  float NdotL = max(dot(N, L), 0.0);

  float shadow = computeShadow(vertIn.shadowCoord, NdotL);
  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;
  // Ambiant = lumière indirecte du ciel, légèrement bleutée. MULTIPLIÉ par la
  // couleur du matériau (et non ajouté) : les zones à l'ombre gardent leur teinte
  // (vert foncé naturel) au lieu d'être délavées vers un gris blanchâtre.
  float3 ambient = float3(0.28, 0.32, 0.40);
  float3 color = material * (ambient + lightCol * NdotL * shadow);

  return float4(color, 1.0);
}
