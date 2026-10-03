import './games.js';

const year = new Date().getFullYear();
document.getElementById('copyright-years').textContent = year > 2026 ? `2026–${year}` : '2026';
