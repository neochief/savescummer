import { artworkClasses, linkClasses } from './gallery-classes.js';

function addImage(card, className, path, defer) {
  const img = document.createElement("img");
  img.className = artworkClasses[className] || className;
  // Omit src until our observer allows the request; native lazy loading preloads too far.
  if (defer) img.dataset.src = `./${path}`;
  else img.src = `./${path}`;
  img.alt = "";
  img.draggable = false;
  img.loading = "eager";
  img.decoding = "async";
  card.append(img);
}

export function fillArtwork(container, game, { defer = false } = {}) {
  container.replaceChildren();
  container.dataset.artStyle = game.artStyle || "";
  if (game.hero) addImage(container, "game-art", game.hero, defer);
  const shade = document.createElement("span");
  shade.className = artworkClasses["game-shade"];
  container.append(shade);
  if (game.logo) addImage(container, "game-logo", game.logo, defer);
  else if (game.artStyle !== "self-contained") {
    const fallback = document.createElement("span");
    fallback.className = artworkClasses["game-fallback"];
    fallback.textContent = game.name;
    container.append(fallback);
  }
}

export function loadArtwork(container) {
  for (const img of container.querySelectorAll('img[data-src]')) {
    img.src = img.dataset.src;
    delete img.dataset.src;
  }
}

export function makeGameLink(game, preview = false, onHover = () => {}) {
  const steam = game.steam != null;
  const link = document.createElement(preview ? "span" : "a");
  link.className = `${preview ? "game-link-preview" : "game-link"} ${linkClasses}`;
  if (!preview) {
    link.href = steam ? `https://store.steampowered.com/app/${game.steam}/` : game.website;
    link.target = "_blank";
    link.rel = "nofollow noopener noreferrer";
    link.setAttribute("aria-label", steam ? `View ${game.name} on Steam` : `Visit ${game.name} website`);
    link.title = steam ? "View on Steam" : "Visit game website";
    link.addEventListener("pointerenter", () => onHover(true));
    link.addEventListener("pointerleave", () => onHover(false));
  }
  const icon = document.createElement("span");
  icon.className = `game-link-icon ${steam ? "steam" : "website"}`;
  icon.setAttribute("aria-hidden", "true");
  const label = document.createElement("span");
  label.textContent = steam ? "Steam" : "Website";
  link.append(icon, label);
  return link;
}
