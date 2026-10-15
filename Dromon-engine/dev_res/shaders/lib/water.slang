// Mer : masque (où y a-t-il de l'eau ?) et absorption sous la surface. Partagé par
// water.slang (surface) et terrain.slang (fond vu à travers l'eau).
#pragma once

#include "bindings.slang"
#include "world_maps.slang"
#include "noise.slang"

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

// Eau vue sur un trajet infini en `xy` : le fond a disparu, reste la lumière diffusée.
// Limite de `underwater` ; couleur des pixels « à l'infini » quand la caméra est sous
// l'eau. Teinte selon la SST : eau froide plus verte (plancton), chaude plus bleue.
float3 deepWaterColor(float2 xy) {
  float warm = smoothstep(ubo.waterSst.x, ubo.waterSst.y, seaSurfaceTemperature(xy));
  return lerp(ubo.waterDeepCold.xyz, ubo.waterDeepColor.xyz, warm) * seaSurfaceLight();
}

// Point où le regard caméra → worldPos traverse le plan de la mer ; `worldPos` s'il
// ne le traverse pas (fragment au-dessus, ou caméra sous l'eau).
float3 seaEntry(float3 worldPos) {
  float3 camPos = ubo.cameraPosition.xyz;
  if (worldPos.z >= seaLevel() || camPos.z <= seaLevel())
    return worldPos;
  float t = (camPos.z - seaLevel()) / (camPos.z - worldPos.z);
  return lerp(camPos, worldPos, t);
}

// Vrai si le regard caméra → worldPos passe sous la surface de la mer. Masque testé au
// point d'entrée, comme la surface et le ciel (sky.slang) : un fond hors du masque vu
// à travers une surface dans le masque est bien sous l'eau.
bool seenThroughSea(float3 worldPos) {
  return worldPos.z < seaLevel() && seaCovers(seaEntry(worldPos).xy);
}

// Fin de la partie « air » du regard caméra → worldPos, là où s'arrête le brouillard
// atmosphérique : la surface de la mer si le fragment est dessous, sinon le fragment.
// Caméra sous l'eau : non géré ici (cf. `cameraUnderSea`).
float3 airPathEnd(float3 worldPos) {
  return seenThroughSea(worldPos) ? seaEntry(worldPos) : worldPos;
}

// Réflectance de l'eau à incidence normale : ((n − 1)/(n + 1))², n = 1,33.
static const float WATER_F0 = 0.02;

// Part de lumière réfléchie (Schlick) : 2 % à la verticale, 100 % en rasant.
float fresnel(float cosTheta) {
  return WATER_F0 + (1.0 - WATER_F0) * pow(1.0 - cosTheta, 5.0);
}

static const float PI = 3.14159265;

// ---------------------------------------------------------------------------------
// Reflet du soleil sur les rides sous-pixel
// ---------------------------------------------------------------------------------

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

// Normale analytique (dérivées des sommes de `gerstnerOffset`). `dx`, `dy` : variation
// de `xy` d'un pixel au voisin (ddx, ddy). ≥ 2 pixels par longueur d'onde (Nyquist),
// sinon la vague scintille et devient de la rugosité.
SeaNormal gerstnerNormal(float2 xy, float2 dx, float2 dy) {
  float coast = coastWaveFactor(xy);
  SeaNormal s;
  float3 n = float3(0.0, 0.0, 1.0);
  s.lostSlopeVariance = 0.0;
  for (int i = 0; i < WAVE_COUNT; i++) {
    float4 w0 = ubo.waves[2 * i];
    // Taille du pixel le long de la direction de la vague seulement : en vue rasante,
    // le pixel est très étiré en profondeur, mais une vague dont les crêtes suivent cet
    // étirement reste résolue.
    float footprint = length(float2(dot(w0.xy, dx), dot(w0.xy, dy)));
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

// ---------------------------------------------------------------------------------
// Écume du rivage
// ---------------------------------------------------------------------------------

// Évaluée par terrain.slang, qui seul connaît la profondeur exacte du fond (la carte
// `coast_distance` est trop imprécise pour un liseré de quelques unités). La profondeur
// sert de distance au rivage. Réglages : `water.foam` (environment.ron), dans l'UBO.
float foamAlbedo() { return ubo.foam[0].x; }

// Arrivée des vagues au rivage, dans [0, 1] : montée rapide (déferlement) puis
// dissolution lente. Phase décalée par un bruit à grande échelle : la côte ne pulse
// pas d'un bloc, la vague semble courir le long de la plage.
float foamSurge(float2 xy, float t) {
  float period = ubo.foam[1].z;
  float spread = ubo.foam[1].w;
  float phase = t / period + 2.0 * valueNoise(xy / spread);
  float s = frac(phase);
  return smoothstep(0.0, 0.08, s) * (1.0 - smoothstep(0.08, 0.85, s));
}

// Couverture d'écume [0, 1] du fond en `worldPos`, vu à travers la surface.
//
// Seuillage d'un bruit par la profondeur : au rivage le seuil est bas (presque tout
// est couvert, sauf quelques trous), au large seuls les pics passent (taches isolées).
float shoreFoam(float3 worldPos) {
  float depth = seaLevel() - worldPos.z;
  // Taille d'un pixel sur le sol. AVANT tout branchement (dérivées).
  float footprint = length(fwidth(worldPos.xy));
  if (depth <= 0.0 || cameraUnderSea() || !seenThroughSea(worldPos)) return 0.0;

  float zoneDepth = ubo.foam[0].y;
  float patchSize = ubo.foam[0].z;
  float bubbleSize = ubo.foam[0].w;
  float bubbleSpeed = ubo.foam[1].x;
  float drift = ubo.foam[1].y;
  float calmZone = ubo.foam[2].x;
  float calmDensity = ubo.foam[2].y;

  float t = ubo.time.x;
  float2 xy = worldPos.xy;
  // Taches : deux octaves de bruit de valeur qui dérivent en sens opposés.
  float2 p = xy / patchSize;
  float patches = 0.65 * valueNoise(p + t * drift / patchSize)
                + 0.35 * valueNoise(p * 2.7 - t * 1.6 * drift / patchSize);
  // Bulles : blanc sur les frontières des cellules, trous au centre.
  float2 w = worley(xy / bubbleSize, t * bubbleSpeed);
  float bubbles = 1.0 - smoothstep(0.0, 0.3, w.y - w.x);
  // Bulles plus petites que ~2 pixels : remplacées par leur moyenne, sinon moiré.
  bubbles = lerp(bubbles, 0.4, saturate(footprint / bubbleSize * 2.0 - 0.5));

  float n = 0.7 * patches + 0.3 * bubbles;
  // La vague élargit la zone et la densifie ; entre deux vagues, il ne reste que les
  // taches les plus denses près du sable.
  float surge = foamSurge(xy, t);
  float zone = zoneDepth * lerp(calmZone, 1.0, surge);
  float density = (1.0 - smoothstep(0.0, zone, depth)) * lerp(calmDensity, 1.0, surge);
  return 0.9 * smoothstep(0.0, 0.2, n + density - 1.0);
}

// Début du fondu du fond vers l'eau profonde, en fraction de la portée horizontale.
static const float FLOOR_FADE_START = 0.8;

// Fond marin vu à travers l'eau (Beer-Lambert). `color` : fond déjà éclairé. Surface
// supposée plane (vagues ignorées).
float3 underwater(float3 color, float3 worldPos) {
  if (!seenThroughSea(worldPos)) return color;
  float depth = seaLevel() - worldPos.z;

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
  float3 deep = deepWaterColor(worldPos.xy);
  float3 scatter = deep * (1.0 - exp(-k * viewPath));

  // Au-delà du terrain chargé, le ciel peint un fond infini (sky.slang) : fondu vers
  // lui à l'approche de la limite, sinon une marche de couleur la trahit.
  float edge = smoothstep(FLOOR_FADE_START, 1.0,
                          distance(worldPos.xy, camPos.xy) / ubo.water.y);
  return lerp(bottom + scatter, deep, edge);
}
