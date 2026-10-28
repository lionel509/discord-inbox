INSERT INTO Calendar (ROWID, title) VALUES (1,'MATH 101 - Work'),(2,'PHYS 102 - Exams'),(3,'CHEM 103 - Lectures'),(4,'CHEM 103 - Exams'),(5,'Home'),(6,'Work'),(7,'CHEM 103 - Prof''s Hours'),(8,'SBU - Lectures'),(9,'SBU - Math');
INSERT INTO CalendarItem (ROWID, summary, start_date, start_tz, all_day, calendar_id, has_recurrences, UUID) VALUES
(1,'HW 3 due',814147200,'_float',1,1,0,'u-math-hw3'),
(2,'✅ HW 2 due',812937600,'_float',1,1,0,'u-math-hw2'),
(3,'Midterm 1',814282200,'America/New_York',0,2,0,'u-phys-mid'),
(4,'Lab 2 report due',814838400,'_float',1,2,0,'u-phys-lab'),
(5,'Personal thing',814060800,'_float',1,5,0,'u-home-1'),
(6,'CHEM 103 Lecture',814802400,'America/New_York',0,3,1,'u-chem-lec'),
(7,'Lab 5 report due',814924800,'_float',1,4,0,'u-chem-lab5'),
(8,'CHEM 103 Midterm',815090400,'America/New_York',0,4,0,'u-chem-mid'),
(9,'Quiz 3 closes',814937400,'America/New_York',0,4,0,'u-chem-q3'),
(10,'Private meeting',814888800,'America/New_York',0,6,0,'u-work-1'),
(11,'Lab week',814924800,'_float',1,4,0,'u-chem-labweek');
INSERT INTO OccurrenceCache (day, event_id, calendar_id, store_id, occurrence_date, occurrence_start_date, occurrence_end_date) VALUES
(814161600,1,1,1,814147200,NULL,NULL),(812952000,2,1,1,812937600,NULL,NULL),(814248000,3,2,1,814282200,NULL,NULL),(814852800,4,2,1,814838400,NULL,NULL),(814075200,5,5,1,814060800,NULL,NULL),
(814766400,6,3,1,814802400,NULL,NULL),(814852800,6,3,1,814888800,NULL,NULL),(814939200,6,3,1,814975200,NULL,NULL),
(814939200,7,4,1,814924800,NULL,NULL),(815025600,8,4,1,815090400,NULL,NULL),(814852800,9,4,1,814937400,NULL,NULL),(814852800,10,6,1,814888800,NULL,NULL),
(814939200,11,4,1,814924800,NULL,NULL),(815025600,11,4,1,814924800,NULL,NULL);
INSERT INTO Calendar (ROWID, title) VALUES (10,'Career'),(11,'Study');
INSERT INTO CalendarItem (ROWID, summary, start_date, start_tz, all_day, calendar_id, has_recurrences, UUID) VALUES
(12,'GitHub Insider',814892400,'America/New_York',0,10,0,'u-career-gh'),
(13,'Study — HSC · CHEM 103 chapter 5',814932000,'America/New_York',0,11,0,'u-study-1'),
(14,'Hackathon week',814752000,'_float',1,10,0,'u-career-hack'),
(15,'Resume review',814897800,'America/New_York',0,10,0,'u-career-cv');
INSERT INTO OccurrenceCache (day, event_id, calendar_id, store_id, occurrence_date, occurrence_start_date, occurrence_end_date) VALUES
(814852800,12,10,1,814892400,NULL,814899600),
(814852800,13,11,1,814932000,NULL,814944600),
(814939200,13,11,1,814939200,814932000,814944600),
(814766400,14,10,1,814766400,NULL,815025599),
(814852800,14,10,1,814852800,814766400,815025599),
(814939200,14,10,1,814939200,814766400,815025599),
(814852800,15,10,1,814897800,NULL,814901400);
