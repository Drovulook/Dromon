// Mer : masque (où y a-t-il de l'eau ?) et absorption sous la surface. Partagé par
// water.slang (surface) et terrain.slang (fond vu à travers l'eau).
#pragma once

#include "bindings.slang"
#include "world_maps.slang"

float seaLevel() { return ubo.water.x; }

// Vrai si la mer recouvre la verticale de `xy`. Même critère au pixel pour la surface
// (discard) et le fond (absorption) : l'un n'est jamais teinté sans l'autre.
bool seaCovers(float2 xy) {
  if (ubo.water.w == 0.0) return false;
  if (distance(xy, ubo.cameraPosition.xy) > ubo.water.y) return false;
  return coastDistance(xy) <= ubo.water.z;
}

// Calculé côté CPU (`Water::update`) : le ciel le lit sans les cartes du monde.
bool cameraUnderSea() { return ubo.waterAbsorption.w != 0.0; }

// Lumière reçue par la surface (face horizontale) : ciel + soleil.
float3 seaSurfaceLight() {
  float3 L = normalize(-ubo.lightDirection.xyz);
  return ubo.ambient.rgb + ubo.lightColor.rgb * ubo.lightColor.w * max(L.z, 0.0);
}

// Eau vue sur un trajet infini : le fond a disparu, reste la lumière diffusée. Limite
// de `underwater` ; couleur des pixels « à l'infini » quand la caméra est sous l'eau.
float3 deepWaterColor() { return ubo.waterDeepColor.xyz * seaSurfaceLight(); }

// Fin de la partie « air » du regard caméra → worldPos, là où s'arrête le brouillard
// atmosphérique : la surface de la mer si le fragment est dessous, sinon le fragment.
// Caméra sous l'eau : non géré ici (cf. `cameraUnderSea`).
float3 airPathEnd(float3 worldPos) {
  float3 camPos = ubo.cameraPosition.xyz;
  if (worldPos.z >= seaLevel() || camPos.z <= seaLevel() || !seaCovers(worldPos.xy))
    return worldPos;
  float t = (camPos.z - seaLevel()) / (camPos.z - worldPos.z);
  return lerp(camPos, worldPos, t);
}

// Réflectance de l'eau à incidence normale : ((n − 1)/(n + 1))², n = 1,33.
static const float WATER_F0 = 0.02;

// Part de lumière réfléchie (Schlick) : 2 % à la verticale, 100 % en rasant.
float fresnel(float cosTheta) {
  return WATER_F0 + (1.0 - WATER_F0) * pow(1.0 - cosTheta, 5.0);
}

static const float PI = 3.14159265;

// Masquage / ombrage de Smith pour Beckmann (approximation de Walter 2007) : part des
// rides vues depuis une direction d'angle `cosTheta`, non cachées par leurs voisines.
// Évite que le reflet n'explose en vue rasante.
float smithG1(float cosTheta, float slopeVariance) {
  float c = max(cosTheta, 1e-4);
  float a = c / (sqrt(slopeVariance * (1.0 - c * c)) + 1e-6); // 1 / (σ · tan θ)
  if (a >= 1.6) return 1.0;
  return (3.535 * a + 2.181 * a * a) / (1.0 + 2.276 * a + 2.577 * a * a);
}

// Reflet du soleil sur les rides sous-pixel : microfacettes, distribution de Beckmann
// (= pentes gaussiennes de Cox-Munk). Radiance = F · D · G · E / (4 · N·V).
// `s2` : variance des pentes non représentées par `N`.
float3 sunGlint(float3 N, float3 toEye, float3 L, float3 sunCol, float s2) {
  float NdotL = dot(N, L);
  float NdotV = dot(N, toEye);
  if (NdotL <= 0.0 || NdotV <= 0.0) return 0.0;

  // Normale qu'une ride doit avoir pour renvoyer le soleil vers l'œil.
  float3 H = normalize(L + toEye);
  float cos2 = max(dot(N, H), 1e-4);
  cos2 *= cos2;
  float D = exp(-(1.0 - cos2) / (cos2 * s2)) / (PI * s2 * cos2 * cos2);
  float G = smithG1(NdotL, s2) * smithG1(NdotV, s2);
  float F = fresnel(saturate(dot(H, toEye)));
  return sunCol * F * D * G / (4.0 * NdotV);
}

// ---------------------------------------------------------------------------------
// Houle de Gerstner
// ---------------------------------------------------------------------------------

// Hauteurs (vertex shader) et normales (fragment shader) sont évaluées séparément, au
// même point de grille `xy` (non déplacé). Chacune efface les vagues trop courtes pour
// sa résolution : la grille de sommets pour les hauteurs, les pixels pour les normales
// — bien plus fins au loin.

// 0 à moins de `LAND_MARGIN` de la côte (crêtes au-dessus des plages), 1 au large.
float coastWaveFactor(float2 xy) {
  float offshore = -coastDistance(xy) - ubo.water.z;
  return smoothstep(0.0, ubo.waveParams.x, offshore);
}

// Phase de la vague `i` au point `xy`.
float wavePhase(int i, float2 xy) {
  float4 w0 = ubo.waves[2 * i];
  float4 w1 = ubo.waves[2 * i + 1];
  return w0.z * dot(w0.xy, xy) - w1.x * ubo.time.x + w1.y;
}

// Déplacement Gerstner du point de grille `xy` : les points décrivent des cercles,
// resserrés sur les crêtes, étirés dans les creux. `cellSize` : espacement réel des
// sommets ; ≥ 4 sommets par longueur d'onde pour représenter une vague.
float3 gerstnerOffset(float2 xy, float cellSize) {
  float coast = coastWaveFactor(xy);
  float3 offset = 0.0;
  for (int i = 0; i < WAVE_COUNT; i++) {
    float4 w0 = ubo.waves[2 * i];
    float fade = smoothstep(4.0, 8.0, 2.0 * PI / (w0.z * cellSize));
    float A = w0.w * coast * fade;
    float theta = wavePhase(i, xy);
    offset += float3(w0.xy * (A * cos(theta)), A * sin(theta));
  }
  return offset;
}

struct SeaNormal {
  float3 normal;            // normalisée
  float lostSlopeVariance;  // pentes des vagues effacées, rendues au reflet du soleil
};

// Normale analytique (dérivées des sommes de `gerstnerOffset`). `footprint` : taille
// d'un pixel sur la surface ; ≥ 2 pixels par longueur d'onde (Nyquist), sinon la
// vague scintille et devient de la rugosité.
SeaNormal gerstnerNormal(float2 xy, float footprint) {
  float coast = coastWaveFactor(xy);
  SeaNormal s;
  float3 n = float3(0.0, 0.0, 1.0);
  s.lostSlopeVariance = 0.0;
  for (int i = 0; i < WAVE_COUNT; i++) {
    float4 w0 = ubo.waves[2 * i];
    float fade = smoothstep(2.0, 4.0, 2.0 * PI / (w0.z * footprint));
    float fullSlope = w0.z * w0.w * coast; // k·A, raideur de la vague
    float kA = fullSlope * fade;
    float theta = wavePhase(i, xy);
    n -= float3(w0.xy * (kA * cos(theta)), kA * sin(theta));
    // Une sinusoïde de raideur k·A a une variance de pente (k·A)² / 2.
    s.lostSlopeVariance += 0.5 * (fullSlope * fullSlope - kA * kA);
  }
  s.normal = normalize(n);
  return s;
}

// Fond marin vu à travers l'eau (Beer-Lambert). `color` : fond déjà éclairé. Surface
// supposée plane (vagues ignorées).
float3 underwater(float3 color, float3 worldPos) {
  float depth = seaLevel() - worldPos.z;
  if (depth <= 0.0 || !seaCovers(worldPos.xy)) return color;

  float3 camPos = ubo.cameraPosition.xyz;
  float3 V = normalize(worldPos - camPos);
  // Trajet du regard sous l'eau : de la surface au fond, ou tout le trajet si la
  // caméra est elle-même sous l'eau.
  float viewPath = camPos.z > seaLevel() ? depth / max(-V.z, 1e-4) : distance(camPos, worldPos);

  float3 k = ubo.waterAbsorption.xyz;
  // Descente de la lumière jusqu'au fond (≈ verticale, profondeur), puis remontée
  // vers l'œil : la couleur du fond est atténuée sur les deux trajets.
  float3 bottom = color * exp(-k * (depth + viewPath));
  // Lumière diffusée par l'eau le long du regard : remplace ce qui a été absorbé.
  float3 scatter = deepWaterColor() * (1.0 - exp(-k * viewPath));
  return bottom + scatter;
}
