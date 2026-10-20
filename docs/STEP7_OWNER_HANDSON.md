# Étape 7 — Liste de contrôle à faire à la main (installateur de test)

# Step 7 — Hands-on checklist for the test installer

Seulement ce que les tests automatiques ne peuvent pas prouver. Chaque étape a été relue contre les écrans actuels de la branche de développement (menus et libellés français réels).

Only what automatic tests cannot prove. Every step was re-read against the current screens (real French labels).

## Avant de commencer / Before you start

1. Installez l'installateur de test (étape 8) et chargez l'usine de démonstration : équipements avec compteurs, plans préventifs, stock dans deux magasins, demandes et OT à plusieurs statuts, et **au moins deux sites** (S1 et S2) avec des équipements installés sur chacun.
2. Créez les comptes ci-dessous dans Utilisateurs & rôles › Utilisateurs › « Nouvel utilisateur ». Les rôles livrés sont « Administrator », « Supervisor », « Planner/Scheduler », « Maintenance Technician ».

| Compte | Rôle | Où il s'applique | À quoi il sert |
| --- | --- | --- | --- |
| Administrateur | Administrator | Toute l'usine | réglages, comptes, listes |
| Superviseur 1 et 2 | Supervisor | Toute l'usine | tri, approbation, permis (droit d'émettre et de clôturer), planification, fiabilité |
| Planificateur | Planner/Scheduler | Toute l'usine | planification, préventif (pas de permis) |
| Planificateur réduit | copie de Planner/Scheduler sans les droits de lire le personnel, les postes, l'organisation et les données de référence | Toute l'usine | étapes de droits réduits |
| Technicien (personne A) | Maintenance Technician | Toute l'usine | demandes, OT, permis (droit d'émettre, pas de clôturer) |
| Magasinier | à créer : droits du stock (voir, gérer, compter, acheter) et voir les OT | Toute l'usine | stock |
| Technicien S1 et Technicien S2 | Maintenance Technician | un seul site chacun | droits par site (un rôle d'administrateur ne peut pas être limité à un site) |

3. Travaillez en français, thème clair ; l'anglais est à la fin. Le thème sombre se règle dans Paramètres › Général › « Apparence » › « Mode de couleur » (administrateurs) ou dans Mon profil (tout le monde) : Clair, Sombre ou Système (par défaut). Le choix est enregistré sur ce poste, pour l'utilisateur Windows. Les écrans de connexion, de verrouillage et d'activation restent toujours en clair. Faites la section L d'abord en clair, puis en sombre.
4. Gardez le Fil d'activité ouvert : Paramètres › « Sécurité & Logs » › « Journal d'activité » › « Fil d'activité » (compte avec le droit paramètres et le droit de lire le journal). Après chaque étape, une ligne lisible doit apparaître.
5. Deux PC (même usine, même serveur) : seulement pour la section K. La section A demande une machine neuve ou remise à zéro.
6. Pour chaque étape : cochez si conforme ; sinon écrivez une ligne : écran · ce que j'ai fait · ce que j'ai vu.

Légende : FR = quoi cliquer ; EN = same in English ; Attendu = résultat attendu ; Rôle = compte et droit nécessaires. Étape facultative = « facultatif » dans le titre.

## A. Machine neuve (premier démarrage) — à faire sur une machine neuve ou remise à zéro

### 1. Premier démarrage et premier administrateur

- **FR :** Entrez la clé de licence (écran d'activation), puis sur la page de connexion cliquez « Créer le premier administrateur » et validez « Créer l'administrateur ». Connectez-vous avec ce compte.
- **EN :** Enter the licence key (activation screen), then on the sign-in page click “Créer le premier administrateur” and confirm. Sign in with that account.
- **Attendu :** La licence est reconnue, le compte est créé et il n'existe aucun compte « admin » intégré. Une fois l'administrateur créé, la page de connexion ne propose plus « Créer le premier administrateur » (à observer : aucun test automatique ne couvre ce point).
- **Expected:** The licence is accepted, the account is created, there is no built-in “admin” account. Once an administrator exists, the sign-in page no longer offers to create the first one (observe only).
- **Rôle :** Aucun compte n'existe encore (personne n'est connecté).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 2. Second administrateur

- **FR :** Le bandeau « Créez un second administrateur » est affiché. Ouvrez Utilisateurs & rôles › Utilisateurs › « Nouvel utilisateur », rôle Administrator, « Où ce rôle s'applique » = toute l'usine. Essayez ensuite de limiter un rôle d'administrateur à un seul site.
- **EN :** The banner “Créez un second administrateur” is shown. Create a second user with the Administrator role for the whole plant, then try to limit an administrator role to one site.
- **Attendu :** Le bandeau disparaît quand un second administrateur actif existe. Un rôle d'administrateur limité à un site est refusé (« Les rôles d'administrateur s'appliquent à toute l'usine… »).
- **Expected:** The banner goes away once a second active administrator exists. A site-limited administrator role is refused.
- **Rôle :** Administrateur (droit de gérer les utilisateurs).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## B. Demande, permis, exécution, clôture (une chaîne, trois personnes)

### 3. Demande de sécurité

- **FR :** Demandes d'intervention › « Nouvelle DI » : choisissez une pompe, cochez « Enjeu sécurité », « Créer la demande ».
- **EN :** Intervention Requests › “Nouvelle DI”: pick a pump, tick “Enjeu sécurité”, “Créer la demande”.
- **Attendu :** La demande apparaît dans la liste (statut « Nouvelle » ou « En attente de tri », file « À trier »). Une ligne lisible apparaît dans le Fil d'activité.
- **Expected:** The request shows in the list (status “Nouvelle” or “En attente de tri”, queue “À trier”). A readable line appears in the activity feed.
- **Rôle :** Technicien.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 4. Tri, approbation, conversion en OT : l'historique nomme qui a décidé

- **FR :** Ouvrez la demande. Faites le tri (« Accepter pour revue » puis « Valider le tri »), puis « Approuver & Convertir en OT » (« Créer l'OT »). Ouvrez l'onglet « Revue ».
- **EN :** Open the request, screen it, then “Approuver & Convertir en OT”. Open the “Revue” tab.
- **Attendu :** L'écran de conversion dit « La demande signalait un risque sécurité : l'OT ne démarrera pas sans permis actif ». Chaque ligne de l'historique de revue porte « Par » suivi du nom de la personne qui a décidé (jamais vide).
- **Expected:** The conversion screen says the OT will not start without an active permit. Every review-history line names the person who decided (“Par …”), never blank.
- **Rôle :** Superviseur 1 (droits de trier, examiner, approuver et convertir les demandes).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 5. Permis LOTO : l'isolement vérifié par une autre personne

- **FR :** Permis de travail › « Nouveau permis » (ou depuis l'OT : « Créer le permis »), type « Consignation / déconsignation (LOTO) », liez l'OT et l'équipement. Personne A : « Ajouter une isolation » (point, type d'énergie, méthode, N° de cadenas), « Marquer posée », puis essayez « Vérifier ». Personne B : « Vérifier ».
- **EN :** Work Permits › “New permit” (or from the OT), LOTO type, link OT and equipment. Person A: add an isolation, “Mark applied”, then try “Verify”. Person B: “Verify”.
- **Attendu :** Pour A : refus lisible (« Vous avez posé cette isolation — une autre personne doit vérifier. »). Pour B : accepté, « Vérifiée par B ».
- **Expected:** For A: refused with a readable reason. For B: accepted, “Verified by B”.
- **Rôle :** A = Technicien, B = Superviseur 1.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 6. Approbation par une autre personne que l'auteur (quatre yeux)

- **FR :** Personne A (qui rédige le permis) : « Soumettre en revue », puis « Approuver ». Ensuite personne B : « Approuver ». Regardez le haut de la fiche du permis.
- **EN :** Person A (author): “Submit for review”, then “Approve”. Then person B: “Approve”. Look at the top of the permit panel.
- **Attendu :** Pour A, qui a rédigé et soumis, refus : « Quatre yeux : vous avez soumis le permis … en revue, une autre personne doit l’approuver. » Pour B, accepté. La fiche affiche « Rédigé par A », « Soumis par A », « Approuvé par B », chacun avec sa date.
- **Expected:** A (who wrote and submitted) is refused with the “you submitted it” reason. B is accepted. The panel shows “Rédigé par”, “Soumis par”, “Approuvé par” with dates.
- **Rôle :** A = Technicien, B = Superviseur 1.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 7. Approbation : ni l'auteur ni la personne qui a soumis (facultatif)

- **FR :** Sur un second permis rédigé par A : le Superviseur 1 « Soumet en revue ». A essaie « Approuver », puis le Superviseur 1 essaie « Approuver », puis le Superviseur 2 « Approuve ».
- **EN :** On a second permit written by A: Supervisor 1 submits. A tries to approve, then Supervisor 1 tries, then Supervisor 2 approves.
- **Attendu :** A est refusé (« Quatre yeux : vous avez rédigé le permis …, une autre personne doit l’approuver. »). Le Superviseur 1 est refusé (« Quatre yeux : vous avez soumis le permis … en revue, une autre personne doit l’approuver. »). Le Superviseur 2 est accepté.
- **Expected:** A is refused (author), Supervisor 1 is refused (submitter), Supervisor 2 is accepted.
- **Rôle :** Superviseur 1 et 2.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 8. Délivrer, activer, fiche LOTO

- **FR :** Sur le permis approuvé : « Délivrer », « Activer », puis « Afficher la fiche » et « Imprimer la fiche LOTO ».
- **EN :** On the approved permit: “Issue”, “Activate”, then show and print the LOTO card.
- **Attendu :** Le permis passe « Délivré » puis « Actif ». La fiche imprimée est lisible sur papier (« DANGER — NE PAS MANŒUVRER »).
- **Expected:** The permit becomes Issued then Active. The printed card reads well on paper.
- **Rôle :** Superviseur 1.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 9. Site sans quatre yeux : auto-approbation signalée

- **FR :** Paramètres › Général › « Règles par site » : choisissez un site, mettez « Autoriser l'auto-approbation des décisions de fiabilité et des permis de travail » sur « Activé » (mot de passe et motif de 10 caractères au moins). La personne A rédige, soumet et approuve un permis de ce site. Remettez ensuite la règle sur « Désactivé » et relisez l'« Historique ».
- **EN :** Settings › General › “Règles par site”: set the self-approval rule On for a site (password and reason). Person A writes, submits and approves a permit of that site. Put the rule back Off and read the History.
- **Attendu :** L'approbation est acceptée et la fiche porte « auto-approbation (site sans 4 yeux) ». Remise à « Désactivé », A est de nouveau refusé. L'Historique donne qui, quand, avant et après, motif, en français correct.
- **Expected:** The self-approval is accepted and flagged “auto-approbation (site sans 4 yeux)”. After switching Off, A is refused again. The History line gives who, when, before/after, reason.
- **Rôle :** Administrateur (droit paramètres) pour la règle ; Technicien pour le permis.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 10. Permis d'avant la mise à jour (seulement si le jeu de démonstration en contient) (facultatif)

- **FR :** Ouvrez un permis écrit avant la mise à jour : s'il est déjà approuvé ou plus loin, regardez « Approuvé par ». S'il est encore brouillon, « Soumettre en revue » ; s'il est en revue, faites « Approuver » par une autre personne que celle qui vient de le soumettre.
- **EN :** Open a permit written before the update: if already approved or further, read “Approuvé par”. If still draft, submit it; if in review, have another person than the submitter try “Approve”.
- **Attendu :** Déjà approuvé : « approbateur non enregistré ». En revue : refus « auteur non enregistré (permis antérieur à la mise à jour) » : il faut l'annuler et établir un nouveau permis.
- **Expected:** Already approved: “approbateur non enregistré”. In review: refused (author not recorded), cancel it and raise a new permit.
- **Rôle :** Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 11. Démarrer l'OT, heures, pièce, coût

- **FR :** Ordres de travail › l'OT : essayez « Démarrer » sans permis actif, puis avec le permis actif. Ajoutez des heures (« Ajouter main-d'œuvre ») et une pièce (« Ajouter une pièce » puis « Ajouter et sortir du stock »).
- **EN :** Work Orders › the OT: try “Démarrer” without an active permit, then with it. Add labour hours and a part issued from stock.
- **Attendu :** Sans permis : refus « Cet OT exige un permis de travail : liez un permis actif avant de le démarrer… ». Avec : l'OT démarre. Le « Résumé des coûts » se met à jour sans recharger.
- **Expected:** Without a permit: refused. With it: starts. The cost summary updates without reloading.
- **Rôle :** Technicien.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 12. Retrait des cadenas, clôture du permis, passation, clôture de l'OT

- **FR :** Sur le permis actif : « Vérifier le retrait » (la boîte « Confirmer le retrait des cadenas » nomme le point, qui l'a posé, qui l'a vérifié) ; l'auteur de la pose A est refusé. Puis « Clôturer le permis », « Enregistrer une passation » (« Remis par (rôle) », « Remis à (rôle) », « Personne qui reçoit », note), « Faire la mainlevée ». Enfin l'OT : « Marquer terminé », « Clôturer ».
- **EN :** On the active permit: “Verify removal” (confirmation names point, who applied, who verified); A, who applied it, is refused. Then close the permit, record the handover (roles, receiver, note), hand back. Finally complete and close the work order.
- **Attendu :** Avant la clôture, la passation dit « La remise se consigne une fois le permis clôturé ». Ensuite le journal montre « Signée par … », « reçue par … » avec date. La personne qui reçoit n'est pas vous et détient le droit de clôture des permis sur ce site. L'activité montre « OT clôturé ».
- **Expected:** Before closing, the handover says it is recorded once the permit is closed. Afterwards the log shows signer and receiver with dates; the receiver is not you. The feed shows the work order closed.
- **Rôle :** Superviseur 1 (droit de clôturer les permis et les OT) ; réceptionnaire = Superviseur 2.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 13. Facultatif : règle des trois personnes d'un type de permis

- **FR :** Permis de travail › « Types de permis » › type LOTO › « Règle des trois personnes : une troisième personne confirme le retrait des cadenas » (désactivée par défaut). Activez-la (mot de passe et motif), puis sur un permis : la personne qui a vérifié l'isolement essaie « Vérifier le retrait ». Remettez la règle sur désactivée.
- **EN :** Work Permits › “Permit types” › LOTO type: switch the three-person lock-removal rule on (password and reason), then the person who verified the isolation tries “Verify removal”. Put it back off.
- **Attendu :** Règle active : la personne qui a vérifié l'isolement est refusée (« Vous avez vérifié cette isolation — une troisième personne doit confirmer son retrait. ») ; une troisième personne est acceptée. Règle désactivée : seule la règle des deux personnes (pas la personne qui a posé) s'applique.
- **Expected:** Rule on: the verifier is refused, a third person is accepted. Rule off: only the two-person rule applies.
- **Rôle :** Administrateur avec droit paramètres et droit d'émettre des permis, pour la règle ; Superviseurs pour le retrait.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 14. Facultatif : ligne d'isolement jamais posée et équipement du permis figé

- **FR :** Sur un permis en brouillon : ajoutez une ligne d'isolement et cliquez « Annuler la ligne » (confirmez). Sur un permis déjà « Approuvé », essayez de changer son équipement.
- **EN :** On a draft permit: add an isolation line and “Annuler la ligne”. On an already approved permit, try to change its equipment.
- **Attendu :** La ligne reste, grisée « Annulée » avec qui et quand, sans pouvoir être posée. Le changement d'équipement est refusé : « Ce permis a été approuvé pour cet équipement. Annulez-le et établissez un nouveau permis. »
- **Expected:** The line stays greyed “Annulée” with who and when. The equipment change is refused.
- **Rôle :** Technicien ou Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## C. Planification et maintenance préventive

### 15. Deux plans préventifs

- **FR :** Maintenance préventive › « Nouveau plan » : un plan « Calendrier fixe » et un plan « Compteur » (équipement avec compteur). Le choix « Équipe » liste les équipes. Proposez, approuvez, puis « Publier » pour générer les échéances.
- **EN :** Preventive Maintenance › “Nouveau plan”: one fixed-calendar plan and one meter plan. The crew picker lists the teams. Propose, approve, publish.
- **Attendu :** Les deux plans sont publiés et génèrent des échéances. Un plan sur compteur demande un équipement ayant un compteur actif.
- **Expected:** Both plans are published and generate due dates. A meter plan needs equipment with an active meter.
- **Rôle :** Planificateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 16. Calendrier préventif

- **FR :** Maintenance préventive › vue « Calendrier » : « Mois », « Semaine », « Jour ».
- **EN :** Preventive Maintenance › “Calendrier” view: month, week, day.
- **Attendu :** Les échéances sont aux bonnes dates ; le changement de mois est rapide.
- **Expected:** Occurrences sit on the right dates; changing month is quick.
- **Rôle :** Planificateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 17. Reporter, manquer, exécuter

- **FR :** Maintenance préventive › « File d'échéances » : « Changer le statut » d'une échéance vers « Reportée » (motif), d'une autre vers « Manquée », d'une troisième vers « Prête à planifier » (crée l'OT préventif).
- **EN :** Preventive Maintenance › “File d'échéances”: change one occurrence to Reportée (reason), one to Manquée, one to Prête à planifier (creates the work order).
- **Attendu :** Chaque action laisse une ligne lisible dans le Fil d'activité ; « Prête à planifier » crée l'OT avec tâches, pièces et exigence de permis du plan.
- **Expected:** Each action leaves a readable feed line; “Prête à planifier” creates the work order.
- **Rôle :** Planificateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 18. Pièces du plan

- **FR :** Dans un plan, nouvelle version › « Pièces requises » › « Ajouter un article » : tapez 2 caractères du code ou du nom.
- **EN :** In a plan's new version › “Pièces requises” › “Ajouter un article”: type 2 characters of the code or name.
- **Attendu :** Le nom et le stock de l'article apparaissent ; chaque pièce se marque « Obligatoire » ou « Optionnelle ».
- **Expected:** The article's name and stock show; each part is marked required or optional.
- **Rôle :** Planificateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 19. Semaine de planification et fenêtres

- **FR :** Planification › « Tableau Gantt » : cliquez une carte « Prêt » du backlog, « Créer l'engagement » (2 OT dans la semaine) et glissez-en un à une autre date. Ajoutez 2 fenêtres de production dans « Fenêtres de planification » (« Ajouter la fenêtre », site « Toute l'usine » puis un site précis).
- **EN :** Planning › Gantt board: click a “Prêt” backlog card, “Créer l'engagement” for 2 work orders, drag one to another day. Add 2 production windows (whole plant, one site).
- **Attendu :** Les engagements et fenêtres apparaissent aux bonnes dates ; une fenêtre verrouillée empêche d'y engager du travail.
- **Expected:** Commitments and windows show on the right dates; a locked window blocks commitments.
- **Rôle :** Planificateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 20. Geler la semaine, break-in, demande de dégel

- **FR :** Planification › « Programme hebdomadaire » : « Approuver » puis « Geler » la semaine. Essayez de déplacer un OT : la boîte « Semaine gelée : entrée par break-in » s'ouvre ; « Enregistrer le break-in » avec « Code motif » et « Approbateur ». Cliquez « Demander le dégel » (« Motif du dégel », « Envoyer la demande »).
- **EN :** Planning › Weekly schedule: approve then freeze the week. Try to move a job: the “frozen week: enter as a break-in” dialog opens; register the break-in with reason code and approver. Click “Demander le dégel”.
- **Attendu :** Le break-in est enregistré (ligne « Intervention imprévue » dans l'activité). Le dégel indique « Il doit être décidé par un autre planificateur » : un autre planificateur voit « Approuver le dégel » / « Refuser le dégel ».
- **Expected:** The break-in is recorded (feed line). The unfreeze request must be decided by another planner.
- **Rôle :** Planificateur ; un second planificateur pour le dégel.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 21. Badge « Arrêt nécessaire » dans le backlog

- **FR :** Ordres de travail › un OT en brouillon ou en planification › cochez « Nécessite un arrêt » (section « Arrêt de l'équipement »). Planification › backlog.
- **EN :** Work Orders › a draft OT › tick “Nécessite un arrêt”. Planning › backlog.
- **Attendu :** La carte de l'OT dans le backlog porte le badge « Arrêt nécessaire » ; une carte sans arrêt n'en porte pas.
- **Expected:** The OT's backlog card shows the “Arrêt nécessaire” badge; a card without shutdown shows none.
- **Rôle :** Planificateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 22. Ligne de pièce prévue : annuler ou rendre optionnelle

- **FR :** Ordres de travail › un OT « En planification » avec des pièces prévues › « Annuler la ligne » (confirmez « Annuler la ligne de pièce »), et basculez une autre ligne entre « Obligatoire » et « Optionnelle ».
- **EN :** Work Orders › a planning-stage OT with planned parts: “Annuler la ligne” (confirm) and switch another line between Obligatoire and Optionnelle.
- **Attendu :** La ligne annulée reste visible « Annulée », hors des totaux et réservations. Une ligne dont une pièce est déjà sortie du magasin ne s'annule pas. Le journal de l'OT dit « Ligne de pièce planifiée annulée » / « rendue optionnelle ».
- **Expected:** The cancelled line stays visible as cancelled, out of totals and reservations. A line with stock already issued cannot be cancelled. The OT log records it.
- **Rôle :** Planificateur ou Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## D. Stock

### 23. Sortie de pièce vers un OT

- **FR :** Pièces de rechange › onglet « Comptoir de sortie » : cherchez l'article, choisissez l'OT, l'emplacement du magasin, validez la sortie.
- **EN :** Spare Parts › “Comptoir de sortie” tab: find the article, pick the OT and the store location, issue it.
- **Attendu :** Le solde baisse dans le bon magasin et le coût apparaît dans le « Résumé des coûts » de l'OT.
- **Expected:** The balance drops in the right store and the cost shows on the work order.
- **Rôle :** Magasinier.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 24. Retour de pièce

- **FR :** Pièces de rechange › « Articles » › l'article › « Ajuster le stock » : type « Entrée », motif de retour, et l'OT dont la pièce revient.
- **EN :** Spare Parts › Articles › the article › “Ajuster le stock”: type Entrée, return reason, and the OT the part comes back from.
- **Attendu :** Le solde remonte dans le bon magasin et l'activité de l'article montre « Retour en stock ».
- **Expected:** The balance goes up in the right store and the article activity shows a stock return.
- **Rôle :** Magasinier.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 25. Transfert de stock entre deux magasins

- **FR :** Pièces de rechange › « Articles » › l'article (qui a du stock disponible dans un magasin) › onglet « Positions de stock » › sur la ligne du magasin de départ, bouton « Transférer ». Choisissez le « Magasin de destination », saisissez une quantité, écrivez le « Motif » (au moins 5 caractères) : la phrase « Transférer N … du magasin … vers … ? » résume le mouvement. Validez avec « Transférer ». Puis : essayez une quantité supérieure à la ligne « Disponible » (stock en magasin moins stock réservé), et un motif de moins de 5 caractères.
- **EN :** Spare Parts › Articles › the article (with available stock in a store) › “Positions de stock” tab › on the line of the source store, “Transférer” button. Choose the “Magasin de destination”, enter a quantity, write the “Motif” (at least 5 characters): the sentence “Transférer N … du magasin … vers … ?” sums up the move. Confirm with “Transférer”. Then try a quantity above the “Disponible” line (stock in store minus reserved stock) and a reason shorter than 5 characters.
- **Attendu :** Le solde du magasin de départ baisse et celui du magasin de destination monte de la même quantité ; une notification de réussite s'affiche ; le stock réservé ne bouge pas et ne peut pas être transféré (la quantité est limitée à « Disponible »). Dans l'activité de l'article, les deux mouvements (sortie et entrée de transfert) portent le motif saisi, l'auteur et la date. Une quantité trop grande ou un motif trop court est refusé avec un message en français. Un solde négatif (possible après une synchronisation) est signalé par le badge « Stock négatif » sur la ligne du magasin et dans la liste des articles (sur le total des magasins).
- **Expected:** The source store balance drops and the destination balance goes up by the same quantity; a success notification shows; reserved stock does not move and cannot be transferred (the quantity is capped at “Disponible”). In the article activity both movements (transfer out and transfer in) carry the reason, the author and the date. A quantity that is too large or a reason that is too short is refused with a translated message. A negative balance (possible after a sync) is flagged with a “Stock négatif” badge on the store line and in the article list (on the total over all stores).
- **Rôle :** Magasinier (droit de gestion du stock, sur toute l'usine). Sans ce droit, le bouton « Transférer » n'apparaît pas. Un utilisateur limité à un site ne peut pas encore transférer (voir « Limites connues »).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 26. Commande en retard

- **FR :** Pièces de rechange › « Achats & réparables » : filtre « En retard (livraison attendue dépassée) » et une commande en retard.
- **EN :** Spare Parts › “Achats & réparables”: the late filter and a late purchase order.
- **Attendu :** « En retard de N jours » est juste ; un brouillon n'est jamais en retard. Facultatif vers minuit UTC (01h00 à Alger) : une commande due aujourd'hui passe en retard à ce moment.
- **Expected:** “Late by N days” is right; a draft is never late. Optional around midnight UTC: a PO due today turns late then.
- **Rôle :** Magasinier.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 27. Écart critique d'un comptage

- **FR :** Paramètres › Général › catégorie « Inventaire » : lisez « Écart critique d'un comptage d'inventaire ». Pièces de rechange › « Inventaires & contrôles » : ouvrez un comptage.
- **EN :** Settings › General › Inventory: read “Écart critique d'un comptage d'inventaire”. Spare Parts › “Inventaires & contrôles”: open a count.
- **Attendu :** Le comptage utilise cette valeur (plus de valeur préremplie modifiable par le compteur). Chaque session garde la tolérance avec laquelle elle a été ouverte.
- **Expected:** The count uses that value (no pre-filled editable value). Each session keeps the tolerance it was opened with.
- **Rôle :** Administrateur (droit paramètres) pour lire le réglage ; Magasinier pour le comptage.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 28. Valorisation et fiche fournisseur

- **FR :** Pièces de rechange › « Valorisation » et la « Fiche de performance » d'un fournisseur (dans « Achats & réparables »).
- **EN :** Spare Parts › “Valorisation” and a supplier's “Fiche de performance”.
- **Attendu :** Chaque chiffre dit sur quelle base il est calculé, jamais d'unités mélangées. Aucun écran n'offre « stock bas seulement ».
- **Expected:** Each figure states its basis, never mixed units. No screen offers “low stock only”.
- **Rôle :** Magasinier ou Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## E. Fiabilité (RAMS)

### 29. Décision de fiabilité approuvée par une seconde personne

- **FR :** Fiabilité (RAMS) › ouvrez une étude › étape « Décisions » : la personne A « Enregistre une décision », la personne B « Approuve » (A voit « Vous l'avez proposée : une autre personne approuve »).
- **EN :** Reliability (RAMS) › open a study › “Décisions”: A records a decision, B approves (A is told another person approves).
- **Attendu :** Statut « Approuvée », approbateur B, une ligne dans l'activité. Sans la règle « site sans quatre yeux », A ne peut pas approuver sa propre décision.
- **Expected:** Status Approuvée, approver B, a feed line. A cannot approve their own decision.
- **Rôle :** A et B sont deux Superviseurs (le technicien et le planificateur livrés peuvent seulement consulter la fiabilité).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 30. Grilles de cotation

- **FR :** Données de référence › « Échelles AMDEC » : changez une grille. Fiabilité (RAMS) › une étude › étape « AMDEC » › « Grilles de cotation ».
- **EN :** Reference Data › FMECA scales: change a rating grid. Reliability › a study › FMECA › rating grids.
- **Attendu :** L'étude utilise la nouvelle grille ; l'avertissement « À revoir » apparaît puis disparaît une fois les lignes revues.
- **Expected:** The study uses the new grid; the “To review” warning appears then clears.
- **Rôle :** Administrateur ou Superviseur : droit de gérer les données de référence ; pour enregistrer les grilles depuis l'étude, droit de gérer la fiabilité (le technicien et le planificateur livrés ne peuvent que les consulter).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 31. Coût d'une réparation repris de Finance

- **FR :** Fiabilité (RAMS) › une étude avec diagramme de fiabilité › étape « Modèle système » › « Simulation de Monte-Carlo » › « Coût d'une réparation » : cliquez « Utiliser le coût correctif moyen de {équipement} ».
- **EN :** Reliability › a study with a reliability diagram › “Modèle système” › Monte Carlo simulation › “Cost of one repair”: click “Utiliser le coût correctif moyen de …”.
- **Attendu :** Le champ se remplit avec la moyenne des OT correctifs clôturés de l'équipement (source, nombre d'OT et devise indiqués ; modifiable). Sans OT correctif clôturé : message « n'a encore aucun OT correctif clôturé ». Sans droit finance : message « demande le droit de consultation des finances ».
- **Expected:** The field fills with the mean of the equipment's closed corrective work orders (source, count, currency shown; editable). With none: a message. Without finance view: a message.
- **Rôle :** Superviseur (voir la fiabilité et les finances).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## F. Équipements

### 32. Remise en service

- **FR :** Équipements › ouvrez un équipement retiré.
- **EN :** Equipment › open a retired asset.
- **Attendu :** Le bouton « Remettre en service » est à côté de l'indication « retiré ». Un compte sans gestion des équipements ne voit pas le bouton. La remise en service demande un statut en service et un motif.
- **Expected:** The “Remettre en service” button sits next to the retired hint. Without equipment management: no button.
- **Rôle :** Superviseur (gère les équipements) ; contre-épreuve : Planificateur (lecture seule).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 33. Onglet Maintenance

- **FR :** Équipements › un équipement › onglet « Maintenance ».
- **EN :** Equipment › an asset › “Maintenance” tab.
- **Attendu :** Le nombre d'OT ouverts porte la mention « Pas encore clôturés », et la phrase « Chiffres de cet équipement seul / et de ses composants » est juste.
- **Expected:** The open count says “Pas encore clôturés”, and the roll-up sentence is right.
- **Rôle :** Technicien.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 34. Assistant d'import

- **FR :** Équipements › « Importer » : un CSV avec 1 ligne en erreur et 1 avertissement (Téléverser › Valider › Aperçu › « Appliquer l'import »). Faites aussi un import de personnel ou d'une liste de référence.
- **EN :** Equipment › “Importer”: a CSV with 1 bad row and 1 warning (upload, validate, preview, apply). Also import personnel or a reference list.
- **Attendu :** Les raisons sont traduites ; « Appliquer l'import » demande confirmation avec les nombres. Notez si les lignes de détail restent dans une seule langue (limite connue).
- **Expected:** Reasons are translated; Apply asks for confirmation with counts. Note whether detail lines stay in one language (known limit).
- **Rôle :** Administrateur ou Superviseur (droit d'importer les équipements).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## G. Personnel, listes de l'usine, données de référence

### 35. Nouvelle valeur « Stage »

- **FR :** Données de référence › « Types d'absence » : « Ajouter une valeur » « Stage », comportement « Formation », puis publiez (« Valider et publier »), sans redémarrer. Même chose pour « Catégories de documents ».
- **EN :** Reference Data › “Types d'absence”: add “Stage”, behaviour Formation, publish, no restart. Same for “Catégories de documents”.
- **Attendu :** « Stage » est offert dans la fenêtre d'absence (Personnel › Disponibilité › « Créer un bloc »), le filtre et la légende ; une absence « Stage » est dessinée comme formation. La catégorie neuve est offerte dans la bibliothèque (Documentation) et l'onglet « Documents et photos » d'un équipement.
- **Expected:** “Stage” is offered in the absence dialog, filter and legend and is drawn as training. The new document category is offered in the library and the asset documents tab.
- **Rôle :** Administrateur (droit de gérer et publier les données de référence).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 36. Marqueur « À revoir »

- **FR :** Cherchez le marqueur ambre « À revoir » sur : fiche personne, dossier, colonne de liste, absence, onglet « Documents et photos », menu de la bibliothèque.
- **EN :** Look for the amber “À revoir” marker on: person card, record, list column, absence, documents tab, library menu.
- **Attendu :** Le marqueur est visible là où une ancienne valeur n'est plus dans la liste de l'usine ; la disponibilité des gens n'a pas changé.
- **Expected:** The marker shows where an old value is no longer in the plant list; nobody's availability changed.
- **Rôle :** Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 37. Statuts à revoir

- **FR :** Personnel : la ligne ambre « Statuts à revoir (N) ». Ouvrez une personne, choisissez un statut, « Enregistrer ».
- **EN :** Personnel: the amber “Statuts à revoir (N)” line. Open a person, choose a status, save.
- **Attendu :** N diminue d'une unité. Un nouveau formulaire (personne, poste, absence) ne présélectionne aucune valeur de liste : il faut choisir.
- **Expected:** N goes down by one. New person/position/absence forms preselect no list value.
- **Rôle :** Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 38. Types d'OT sans nature

- **FR :** Données de référence : en haut de la page, la ligne « N types d'OT n'ont pas de nature de maintenance ». Cliquez « Attribuer une nature », choisissez la nature, écrivez un motif de 10 caractères au moins, « Enregistrer ».
- **EN :** Reference Data: at the top, “N work-order types have no maintenance nature”. Click “Attribuer une nature”, pick one, write a reason, save.
- **Attendu :** Le type disparaît de la liste ; quand il n'y en a plus, la ligne disparaît. Sans nature, un type ne compte dans aucune part (planifiée, réactive, urgence) de l'accueil. Si aucun type n'est sans nature, la ligne n'existe pas : c'est normal.
- **Expected:** The type leaves the list; when none is left, the banner disappears. If no type lacks a nature, the banner is not shown: that is normal.
- **Rôle :** Administrateur ou Superviseur (droit de gérer les données de référence).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## H. Accueil et petits points

### 39. Chiffres de l'accueil

- **FR :** Vue d'ensemble › panneau « Supervision » › « Arriéré OT » : cliquez « À réaliser », « En retard », un de « Planifié / Réactif / Urgence », puis « Stock à réapprovisionner » (« Articles sous le seuil »).
- **EN :** Overview › “Supervision” panel › “Arriéré OT”: click To do, Overdue, one of Planned/Reactive/Emergency, then “Stock à réapprovisionner”.
- **Attendu :** La liste s'ouvre avec une puce de filtre et le même nombre que le chiffre cliqué. Le nombre « Permis ouverts » exclut les permis hors de vos sites.
- **Expected:** The list opens with a filter chip and the same number as the figure. “Permis ouverts” leaves out permits outside your sites.
- **Rôle :** Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 40. Messages de Réglages

- **FR :** Paramètres › Général : enregistrez 2 ou 3 réglages.
- **EN :** Settings › General: save 2 or 3 settings.
- **Attendu :** La confirmation apparaît en bas de l'application, pas dans la page Paramètres.
- **Expected:** The confirmation appears at the bottom of the app, not inside Settings.
- **Rôle :** Administrateur (droit paramètres).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 41. Vue du journal d'activité

- **FR :** Paramètres › « Sécurité & Logs » › « Journal d'activité » › « Fil d'activité » : filtre « Unité d'organisation », nom de la vue, « Enregistrer la vue », rechargez l'application.
- **EN :** Settings › “Sécurité & Logs” › Activity log › “Fil d'activité”: pick an organisation unit, name the view, save, reload.
- **Attendu :** L'unité choisie est conservée dans « Vues enregistrées ».
- **Expected:** The unit is kept in the saved views.
- **Rôle :** Administrateur (droit paramètres et lecture du journal). L'ancien chemin « Administration › Activité » n'existe plus.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 42. Afficher plus dans un tableau

- **FR :** Ordres de travail ou Demandes d'intervention › vue « Kanban » : mettez un filtre, puis « Charger plus (N restants) » (OT) ou « Afficher N de plus » (demandes) dans une colonne.
- **EN :** Work Orders or Requests › Kanban: set a filter, then load more in a lane.
- **Attendu :** 20 éléments de plus apparaissent et respectent le filtre.
- **Expected:** 20 more appear and match the filter.
- **Rôle :** Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 43. Cockpit budget

- **FR :** Budget & coûts › onglet « Pilotage » : ouvrez une ligne avec beaucoup d'écritures. Cliquez une ligne juste après l'ouverture du cockpit.
- **EN :** Budget & Costs › “Pilotage” tab: open a line with many postings. Click a line right after the cockpit opens.
- **Attendu :** « Total des N écritures » est juste ; « Charger la suite » ajoute des lignes jusqu'à la fin ; le détail s'ouvre au premier clic.
- **Expected:** “Total des N écritures” is right; “Charger la suite” adds lines to the end; the detail opens on the first click.
- **Rôle :** Superviseur ou Finance Manager.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 44. Changer de langue sur une liste

- **FR :** Sur une liste avec une ligne choisie, menu utilisateur › « Langue » : changez de langue.
- **EN :** On a list with a row selected, user menu › “Langue”: switch language.
- **Attendu :** Les lignes et la sélection restent.
- **Expected:** Rows and selection stay.
- **Rôle :** Tout compte.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 45. Code PIN de verrouillage

- **FR :** Paramètres › « Mon profil » : définissez un PIN. Verrouillez la session, saisissez de mauvais PIN jusqu'au blocage.
- **EN :** Settings › “Mon profil”: set a PIN. Lock the session, enter wrong PINs until disabled.
- **Attendu :** Message « Trop de tentatives PIN. Utilisez votre mot de passe. » et bascule vers « Utiliser le mot de passe ».
- **Expected:** A clear message and a switch to password unlock.
- **Rôle :** Tout compte.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 46. Formulaires standard

- **FR :** Ouvrez : Formations & habilitations › « Sessions de formation » › « Nouvelle session » ; « Catalogue » › « Nouvelle habilitation » ; « Exigences des travaux » › « Nouvelle exigence » ; Vue d'ensemble › « Personnaliser » ; Analytique › « Personnaliser ».
- **EN :** Open: Training › new session; Catalogue › new certification; Requirements › new requirement; Overview › Customise; Analytics › Customise.
- **Attendu :** Chaque fenêtre a un titre, des blocs titrés, un pied de page fixe avec « Annuler » puis le bouton principal. Un clic à côté ne ferme pas la fenêtre ; × et Échap la ferment sans rien enregistrer.
- **Expected:** Each dialog has a title, titled blocks and a sticky footer (Cancel, then the main button). Clicking outside does not close it; × and Esc close without saving.
- **Rôle :** Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## I. Droits par site et droits réduits

### 47. Utilisateur limité à un site

- **FR :** Utilisateurs & rôles › Utilisateurs › « Nouvel utilisateur » : rôle Maintenance Technician, « Où ce rôle s'applique » = le site S1 (un second compte pour S2). Connectez-vous avec le compte S1 : Demandes d'intervention, Ordres de travail, Permis de travail, Vue d'ensemble.
- **EN :** Users & Roles › “Nouvel utilisateur”: Technician role limited to site S1 (a second account for S2). Sign in as S1 and open requests, work orders, permits, overview.
- **Attendu :** Les listes ne montrent que le site S1 et disent « N demandes (OT, permis) ne sont pas affichés : hors de vos sites ». Les chiffres de l'accueil ne comptent que S1. Créer une demande ou agir sur un enregistrement de S2 est refusé : « Cet enregistrement appartient à un site hors de vos rôles. »
- **Expected:** Lists show only S1 and say how many records are outside your sites. Home figures count S1 only. Acting on an S2 record is refused.
- **Rôle :** Administrateur pour créer ; compte limité à S1 pour l'épreuve.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 48. Utilisateur de toute l'usine

- **FR :** Connectez-vous avec un Superviseur « Toute l'usine » et ouvrez les mêmes écrans que l'étape précédente.
- **EN :** Sign in as a plant-wide Supervisor and open the same screens.
- **Attendu :** Aucun avertissement « hors de vos sites » ; les enregistrements de S1 et de S2 sont tous visibles. Les enregistrements marqués « À revoir : sans site » ne sont visibles que de lui.
- **Expected:** No “outside your sites” warning; S1 and S2 records are all visible. Records marked “sans site” are visible only to plant-wide users.
- **Rôle :** Superviseur « Toute l'usine ».
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 49. Planificateur à droits réduits

- **FR :** Connectez-vous en planificateur sans droit personnel ni organisation (copie de Planner/Scheduler sans les droits de lire le personnel, l'organisation et les données de référence). Ouvrez Planification (fenêtre de production), la planification d'un OT, un plan préventif, le choix d'un équipement dans une demande, un formulaire de personne.
- **EN :** Sign in as a planner without personnel or organisation rights. Open Planning (production window), an OT's planning, a PM plan, an asset picker in a request, a person form.
- **Attendu :** Chaque liste manquante montre une petite note « … demande le droit de consultation … », jamais une liste vide trompeuse ; la fenêtre de production n'offre que « Toute l'usine » avec une note. Ni fenêtre d'erreur, ni écran de verrouillage.
- **Expected:** Each missing list shows a small “right required” note, never a misleading empty list; the production window offers only “Toute l'usine” with a note. No error popup, no lock screen.
- **Rôle :** Planificateur à droits réduits.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 50. Étiquette d'absence neutre du planificateur

- **FR :** Avec le planificateur à droits réduits : Planification › « Tableau Gantt » (et la fenêtre « Engager au planning ») sur un technicien absent. Puis avec le planificateur livré (qui lit le personnel).
- **EN :** As the reduced planner: Planning › Gantt board (and the commit dialog) on an absent technician. Then as the shipped planner.
- **Attendu :** Le planificateur réduit voit seulement « Absent » (ou « Absent — critique pour la planification »), jamais le motif. Le planificateur avec droit personnel voit le type d'absence nommé.
- **Expected:** The reduced planner sees only “Absent” (or critical), never the reason. The planner with personnel rights sees the absence type.
- **Rôle :** Planificateur réduit puis Planificateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 51. Facultatif : enregistrements sans site

- **FR :** Organisation : si le bouton « Enregistrements sans site (N) » existe, ouvrez-le et donnez un site à un ancien enregistrement (motif de 10 caractères au moins).
- **EN :** Organisation: if “Enregistrements sans site (N)” exists, open it and give an old record a site (reason required).
- **Attendu :** Un compte limité à un site est refusé ; un administrateur d'organisation de toute l'usine y arrive. Un enregistrement qui a déjà reçu un site n'est plus proposé.
- **Expected:** A site-limited account is refused; a plant-wide organisation administrator can do it. A record already given a site is not offered again.
- **Rôle :** Administrateur de l'organisation, toute l'usine.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## J. Organisation et sécurité du poste

### 52. Boucle dans l'organisation

- **FR :** Organisation › « Règles de hiérarchie » (mode « Brouillon ») › « Règles parent → enfant » : ajoutez A → B puis B → A, puis publiez.
- **EN :** Organisation › “Règles de hiérarchie” (draft mode): add A → B then B → A, then publish.
- **Attendu :** La publication est refusée (boucle). Un type sous le même type (A → A) est permis.
- **Expected:** Publishing is refused (loop). A type under the same type is allowed.
- **Rôle :** Administrateur de l'organisation.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 53. Test de politique d'accès

- **FR :** Paramètres › « Sécurité & Logs » › politique de session : « Modifier le brouillon », puis « Tester le brouillon ».
- **EN :** Settings › “Sécurité & Logs” › session policy: edit the draft, then test it.
- **Attendu :** Les « Résultats des tests » sont en français ; les champs et plages sont nommés (par exemple « Délai d'inactivité (minutes) »).
- **Expected:** The test results are in French; fields and ranges are named.
- **Rôle :** Administrateur (droit paramètres).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 54. Verrouillage après inactivité

- **FR :** Même panneau : baissez « Délai d'inactivité (minutes) » dans le brouillon, testez, « Activer » (mot de passe), puis laissez l'écran se verrouiller.
- **EN :** Same panel: lower the idle time in the draft, test, activate, let the screen lock.
- **Attendu :** « Session verrouillée » s'affiche sur un arrière-plan assombri ; « Déverrouiller » redonne l'accès. (Le point visuel est l'étape 63.)
- **Expected:** “Session verrouillée” shows over a dimmed background; unlock works.
- **Rôle :** Administrateur (droit paramètres).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 55. Licence (lecture)

- **FR :** Paramètres › « Système & Sync » › « Licence ». Ouvrez la ligne « événements de licence récents (détails techniques) ».
- **EN :** Settings › “Système & Sync” › Licence. Open the “recent licence events (technical details)” line.
- **Attendu :** L'état de la licence est lisible (édition, activation, ce poste). Les actions de licence (Suspendre, Révoquer) demandent un administrateur : ne pas les lancer sur le poste de test sans besoin.
- **Expected:** The licence state is readable. Licence actions need an administrator: do not run them on the test PC without reason.
- **Rôle :** Administrateur (droit paramètres).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## K. Deux PC (même usine, même serveur de synchronisation)

### 56. Règle par site et avis aux administrateurs

- **FR :** PC 1 : Paramètres › Général › « Règles par site », site S1 › « Activé » (mot de passe, motif). Synchronisez. PC 2 : connectez-vous avec un administrateur : un avis « Règle des quatre yeux désactivée à S1 sur un autre poste » est affiché en haut ; essayez de le masquer, puis cliquez « J'ai pris connaissance ». Relisez l'« Historique » de la règle.
- **EN :** PC 1: Settings › General › Rules per site, S1 › On (password, reason). Sync. PC 2: sign in as an administrator: a notice “Règle des quatre yeux désactivée à S1 sur un autre poste” is shown; try to hide it, then acknowledge it.
- **Attendu :** Sur PC 2, S1 est « Activé » et le changement de PC 1 est dans l'Historique, sans conflit. Quand la règle « retrait des cadenas à trois personnes » d'un type de permis est activée ou désactivée sur un autre PC, un avis du même genre arrive (« Retrait des cadenas à trois personnes activé pour … sur un autre poste » / « désactivé »). Chaque avis reste affiché à chaque connexion jusqu'à la confirmation (impossible à archiver ou reporter avant) ; il est aussi dans la boîte de réception de Notifications. Il n'est jamais envoyé par e-mail. Remettez ensuite la règle sur « Désactivé » (un second avis « réactivée » arrive).
- **Expected:** On PC 2, S1 is On and PC 1's change is in the History, no conflict. The notice stays on every sign-in until acknowledged, cannot be archived or snoozed before, and is never emailed. Switch it Off again: a second notice (re-enabled) arrives.
- **Rôle :** Administrateur (droit paramètres) sur les deux PC.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 57. Semaine gelée

- **FR :** PC 1 : Planification › « Programme hebdomadaire » : « Approuver » puis « Geler » une semaine. Synchronisez. PC 2 : même écran.
- **EN :** PC 1: Planning › Weekly schedule: approve then freeze a week. Sync. PC 2: same screen.
- **Attendu :** PC 2 montre « Gelé » avec l'historique de PC 1.
- **Expected:** PC 2 shows “Gelé” with PC 1's history.
- **Rôle :** Planificateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 58. Liste modifiée

- **FR :** PC 1 : Données de référence › modifiez une liste et publiez. PC 2 : ouvrez un écran qui l'utilise (copie de plus de 5 minutes, ou après redémarrage).
- **EN :** PC 1: Reference Data: change a list and publish. PC 2: open a screen using it (copy older than 5 minutes, or after restart).
- **Attendu :** La nouvelle valeur est là sur PC 2.
- **Expected:** The new value is there on PC 2.
- **Rôle :** Administrateur (droit de gérer et publier les données de référence).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 59. Carte de conflit

- **FR :** Modifiez le même enregistrement sur les deux PC hors ligne, puis synchronisez. Ouvrez le nuage de la barre du haut : « Centre de synchronisation » › « Conflits non résolus ».
- **EN :** Edit the same record on both PCs offline, then sync. Open the top-bar cloud: Sync center › Unresolved conflicts.
- **Attendu :** Le nuage s'anime en rouge ; la carte nomme l'enregistrement, met en évidence le bouton « Recommandé : … » et demande confirmation avant « Prendre la version du serveur ». La dernière carte résolue disparaît et le nuage se calme.
- **Expected:** The cloud pulses red; the card names the record, highlights the recommended button, and asks to confirm before taking the server version. The last resolved card disappears; the cloud calms.
- **Rôle :** Administrateur (droit de résoudre les conflits).
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 60. Facultatif : décision auto-approuvée hors ligne

- **FR :** PC 2 hors ligne passe S1 à « Désactivé » ; PC 1 (« Activé ») fait approuver une décision de fiabilité par son auteur ; synchronisez.
- **EN :** PC 2 offline sets S1 Off; PC 1 (On) has a decision self-approved; sync.
- **Attendu :** La décision n'est pas appliquée sur PC 2 ; un conflit « Une décision approuvée par la personne qui l'a proposée (quatre yeux) » apparaît ; S1 reste « Désactivé » des deux côtés.
- **Expected:** The decision is not applied on PC 2; a four-eyes conflict appears; S1 stays Off on both.
- **Rôle :** Administrateur et Superviseur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 61. Facultatif : permis auto-approuvé reçu après retour aux quatre yeux

- **FR :** PC 1 (site S1 « Activé ») : l'auteur rédige, soumet et approuve un permis. PC 2 : S1 est « Désactivé ». Synchronisez et ouvrez « Conflits non résolus » sur PC 2.
- **EN :** PC 1 (S1 On): the author writes, submits and approves a permit. PC 2: S1 is Off. Sync and open the conflicts on PC 2.
- **Attendu :** Un conflit « Un permis approuvé sur un autre poste par la personne qui l'a rédigé ou soumis… » apparaît sur PC 2 ; le permis n'y est pas appliqué.
- **Expected:** A conflict about a permit approved by its author or submitter appears on PC 2; the permit is not applied there.
- **Rôle :** Technicien + Administrateur.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 62. Facultatif : ligne de pièce annulée sur un autre poste

- **FR :** PC 1 : annulez une ligne de pièce planifiée (comme à l'étape 22). PC 2 (hors ligne avant) : sortez du magasin une pièce pour cette ligne. Synchronisez.
- **EN :** PC 1: cancel a planned part line. PC 2 (offline before): issue a part from the store for that line. Sync.
- **Attendu :** Un conflit « Une ligne de pièce planifiée a été annulée sur un autre poste, mais des pièces ont déjà été utilisées ou sorties… » demande de vérifier avec le planificateur et le magasin.
- **Expected:** A conflict says the line was cancelled elsewhere but parts were already used or issued here.
- **Rôle :** Planificateur + Magasinier.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## L. Passage visuel (chaque point en clair, puis en sombre)

Pour le sombre : Paramètres › Général › « Apparence » › « Mode de couleur » › « Sombre » (ou Mon profil). Le badge « Enregistré sur ce poste » confirme que le choix est gardé sur ce PC. Gardez « Sombre » pour les cases « Sombre OK » ci-dessous, puis remettez « Clair » ou « Système » si vous voulez.

For dark: Settings › General › “Apparence” › “Mode de couleur” › “Sombre” (or My profile). The badge “Enregistré sur ce poste” confirms the choice is kept on this PC. Keep “Sombre” for the dark boxes below, then switch back if you like.

### 63. Verrouillage

- **FR :** Après l'étape 54 : l'arrière-plan est assombri (ni noir, ni clair) et la carte est lisible.
- **EN :** After step 54: the background is dimmed (neither black nor clear), the card readable.
- **Attendu :** Correct en thème clair et en thème sombre (texte lisible, rien de blanc éclatant ni d'illisible).
- **Expected:** Correct in the light and the dark theme (readable text, nothing glaring or unreadable).
- **Rôle :** Tout compte.
- [ ] Clair OK  - [ ] Sombre OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 64. Cartes

- **FR :** Vue d'ensemble, Analytique, Équipements, Planification : cartes à teinte légère, un peu plus sombres au survol.
- **EN :** Overview, Analytics, Equipment, Planning: cards have a light tint and darken slightly on hover.
- **Attendu :** Correct en thème clair et en thème sombre (texte lisible, rien de blanc éclatant ni d'illisible).
- **Expected:** Correct in the light and the dark theme (readable text, nothing glaring or unreadable).
- **Rôle :** Tout compte.
- [ ] Clair OK  - [ ] Sombre OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 65. Message d'erreur

- **FR :** Clôturez un OT avec une tâche obligatoire ouverte : le message est sur fond rouge doux, texte lisible.
- **EN :** Close a work order with a mandatory task open: soft red background, readable text.
- **Attendu :** Correct en thème clair et en thème sombre (texte lisible, rien de blanc éclatant ni d'illisible).
- **Expected:** Correct in the light and the dark theme (readable text, nothing glaring or unreadable).
- **Rôle :** Tout compte.
- [ ] Clair OK  - [ ] Sombre OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 66. Ligne survolée et choisie

- **FR :** Liste des OT : une ligne survolée a une teinte légère, la ligne choisie se distingue.
- **EN :** Work-order list: hovered row has a faint tint, the selected row stands out.
- **Attendu :** Correct en thème clair et en thème sombre (texte lisible, rien de blanc éclatant ni d'illisible).
- **Expected:** Correct in the light and the dark theme (readable text, nothing glaring or unreadable).
- **Rôle :** Tout compte.
- [ ] Clair OK  - [ ] Sombre OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 67. Licence

- **FR :** Paramètres › Système & Sync › Licence › « événements de licence récents (détails techniques) » : la boîte a une teinte légère, texte lisible.
- **EN :** Settings › System & Sync › Licence › technical details box: light tint, readable text.
- **Attendu :** Correct en thème clair et en thème sombre (texte lisible, rien de blanc éclatant ni d'illisible).
- **Expected:** Correct in the light and the dark theme (readable text, nothing glaring or unreadable).
- **Rôle :** Tout compte.
- [ ] Clair OK  - [ ] Sombre OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 68. Lignes d'import

- **FR :** Pendant l'import d'équipements : erreurs en rouge, avertissements en ambre, lignes valides sans teinte.
- **EN :** During the equipment import: errors red, warnings amber, valid rows plain.
- **Attendu :** Correct en thème clair et en thème sombre (texte lisible, rien de blanc éclatant ni d'illisible).
- **Expected:** Correct in the light and the dark theme (readable text, nothing glaring or unreadable).
- **Rôle :** Tout compte.
- [ ] Clair OK  - [ ] Sombre OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 69. Calendrier de date

- **FR :** Dans un sélecteur de date : jours hors du mois en gris, jours désactivés encore plus pâles.
- **EN :** In any date picker: days outside the month grey, disabled days fainter.
- **Attendu :** Correct en thème clair et en thème sombre (texte lisible, rien de blanc éclatant ni d'illisible).
- **Expected:** Correct in the light and the dark theme (readable text, nothing glaring or unreadable).
- **Rôle :** Tout compte.
- [ ] Clair OK  - [ ] Sombre OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 70. Facultatif : le choix « Système » suit Windows

- **FR :** Mode de couleur › « Système ». Changez le mode de couleur de Windows (Paramètres Windows › Personnalisation › Couleurs : clair puis sombre). Ouvrez ensuite l'écran de connexion (déconnexion).
- **EN :** Colour mode › “Système”. Change the Windows colour mode (light then dark). Then open the sign-in screen (sign out).
- **Attendu :** L'application suit Windows sans redémarrage (« Affichage actuel » change). L'écran de connexion reste clair dans tous les cas. Le choix est propre à ce poste : un autre PC garde le sien.
- **Expected:** The app follows Windows without a restart (“Affichage actuel” changes). The sign-in screen stays light in every mode. The choice is per PC.
- **Rôle :** Tout compte.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## M. Anglais et journée réaliste

### 71. Coup d'œil en anglais

- **FR :** Passez en anglais (menu utilisateur › « Langue ») et reparcourez : demande, permis, OT, accueil, nouvelle valeur « Stage ».
- **EN :** Switch to English and go through: request, permit, work order, overview, the new “Stage” value.
- **Attendu :** Aucun texte français ni code technique ne reste visible.
- **Expected:** No French text or technical code is left showing.
- **Rôle :** Tout compte.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

### 72. Une journée réaliste

- **FR :** Pour chaque module ancien (stock, fiabilité, préventif, analytique, finance, personnel, « Rondes et checklists ») faites une journée de travail normale et notez toute limite ou gêne.
- **EN :** For each earlier module (stock, reliability, PM, analytics, finance, personnel, inspection rounds) do one normal working day and note every limit or annoyance.
- **Attendu :** Rien de bloquant ; tout ce qui gêne est noté avec écran, action, constat.
- **Expected:** Nothing blocking; every annoyance noted.
- **Rôle :** Selon le module.
- [ ] OK
- Si pas conforme, écrire : écran · ce que j'ai fait · ce que j'ai vu : ______

## Étapes de l'ancienne liste qui n'existent plus

- Ancienne étape 36 « Voyant base de données en mode client » : il n'y a pas de mode client avec liaison serveur dans cette version ; la base est locale et le voyant « Base de données » de la barre d'état ne teste que cette base locale (états « OK », « ne répond pas à temps », « erreur » prouvés par des tests automatiques). À retirer de la recette à la main.
- Ancien « transfert entre deux magasins » (ancienne étape 12) : il a maintenant son écran (étape 25).
- « Entrée en force » (ancienne étape 10) s'appelle maintenant « break-in » ; « En traitement » (ancienne étape 1) n'est plus un statut.

---

Total : 72 étapes (dont 9 facultatives).
